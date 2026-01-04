import asyncio
import json
import logging
import uuid
from asyncio import Event, Queue
from contextlib import asynccontextmanager
from dataclasses import asdict, dataclass
from datetime import date, datetime
from pathlib import Path
from typing import Literal

import aiohttp
from anyio import TemporaryDirectory
from pydantic import TypeAdapter, ValidationError
from pydantic_settings import BaseSettings, SettingsConfigDict
from starlette.applications import Starlette
from starlette.routing import Mount, WebSocketRoute
from starlette.staticfiles import StaticFiles
from starlette.websockets import WebSocket, WebSocketDisconnect

import stereo.db as db
import stereo.lib as lib
from stereo._logging import setup_logger
from stereo._version import __version__
from stereo.lib import Collection, Track
from stereo.message import (
    MsgAddTrack,
    MsgAddTracks,
    MsgBackendInfo,
    MsgCheckImportFrom,
    MsgClient,
    MsgCollectionContainsId,
    MsgCollectionContainsIdResponse,
    MsgCollectionInfo,
    MsgCreateCollection,
    MsgCreateYTAnonPlaylist,
    MsgDefaultCollection,
    MsgDeleteTracks,
    MsgGetPathCompletions,
    MsgGetRows,
    MsgGetTrackInfo,
    MsgHeartbeat,
    MsgImportFrom,
    MsgImportFromValid,
    MsgIncPlayCount,
    MsgNotification,
    MsgPathCompletions,
    MsgReloadTracks,
    MsgRows,
    MsgSearch,
    MsgSearchCancelAll,
    MsgSearchComplete,
    MsgSearchResult,
    MsgServer,
    MsgSetCollection,
    MsgTrackInfo,
    MsgTrackUpdate,
    MsgUpdateRating,
    MsgUpdateTrack,
    MsgValidateTrack,
    MsgValidateTrackReply,
    MsgYTAnonPlaylist,
)
from stereo.utils import download_file, get_path_completions, is_file_or_url


class JsonEnc(json.JSONEncoder):
    def default(self, o):
        if isinstance(o, (datetime, date)):
            return o.isoformat()
        if isinstance(o, Track):
            return o.model_dump()
        if isinstance(o, Path):
            return str(o)
        return super().default(o)


class Settings(BaseSettings):
    home: Path = Path.home() / ".local" / "share" / "stereo"
    verbosity: int = 0
    dev: bool = False

    model_config = SettingsConfigDict(env_prefix="STEREO_")


@dataclass
class SessionState:
    collection: Collection | None = None
    search_task: asyncio.Task[None] | None = None

    def db_ctx(self) -> db.Context | None:
        if self.collection is not None:
            return db.Context(self.collection.path)
        else:
            return None


qs_tx: dict[str, Queue[MsgServer]] = {}  # WS send queues
qs_rx: dict[str, Queue[MsgClient]] = {}  # WS receive queues

settings = Settings()

logger = logging.getLogger(__name__)

client_msg_adapter = TypeAdapter(MsgClient)


def decode_client_msg(msg: str) -> MsgClient:
    return client_msg_adapter.validate_json(msg)


async def ws_broadcast(msg: MsgServer):
    for _, q in qs_tx.items():
        await q.put(msg)


async def websocket_endpoint(websocket: WebSocket):
    await websocket.accept()
    uid = str(uuid.uuid4())

    q_tx: Queue[MsgServer] = asyncio.Queue(10000)
    q_rx: Queue[MsgClient] = asyncio.Queue(10000)

    qs_tx[uid] = q_tx
    qs_rx[uid] = q_rx

    default_collection_path = settings.home / "stereo.db"

    await db.init_db(db.Context(default_collection_path))

    n = await db.count_rows(db.Context(default_collection_path), {})

    default_collection = Collection(default_collection_path, n)

    ev_state_change = Event()

    state = SessionState()

    logger.info(f"Opening new WS connection: {uid}")

    async def notify_client(
        msg: str, kind: Literal["info", "warn", "warning", "error"]
    ) -> None:
        await q_tx.put(MsgNotification(msg, kind))

    async def search_fuzzy(query: str, query_id: int, limit: int):
        async with aiohttp.ClientSession() as session:
            i = 0
            try:
                async for track in lib.search_fuzzy(session, query):
                    await q_tx.put(MsgSearchResult(query_id, track))
                    i += 1
                    if i >= limit:
                        break
            except Exception as ex:
                logger.exception("search_fuzzy failed")
                await notify_client(f"Search failed with exception: {ex}", "error")

            finally:
                await q_tx.put(MsgSearchComplete(query_id))

    async def search_by_artist(query: str, query_id: int, limit: int):
        async with aiohttp.ClientSession() as session:
            i = 0
            try:
                async for track in lib.get_artist_releases(session, query):
                    await q_tx.put(MsgSearchResult(query_id, track))
                    i += 1
                    if i >= limit:
                        break
            except Exception as ex:
                logger.exception("search_by_artist failed")
                await notify_client(f"Search failed with exception: {ex}", "error")

            finally:
                await q_tx.put(MsgSearchComplete(query_id))

    async def search_by_label(query: str, query_id: int, limit: int):
        async with aiohttp.ClientSession() as session:
            i = 0
            try:
                async for track in lib.get_label_releases(session, query):
                    await q_tx.put(MsgSearchResult(query_id, track))
                    i += 1
                    if i >= limit:
                        break
            except Exception as ex:
                logger.exception("search_by_label failed")
                await notify_client(f"Search failed with exception: {ex}", "error")

            finally:
                await q_tx.put(MsgSearchComplete(query_id))

    async def update_collection() -> None:
        if state.collection is not None:
            path = state.collection.path
            n = await db.count_rows(db.Context(Path(path)), {})
            state.collection.size = n
            await q_tx.put(MsgCollectionInfo(collection=state.collection))
            ev_state_change.set()

    async def handle_client_msg(msg: MsgClient):
        match msg:
            case MsgHeartbeat(t):
                await q_tx.put(MsgHeartbeat(t))

            case MsgValidateTrack(id, data):
                try:
                    t = Track(**data)
                    await q_tx.put(MsgValidateTrackReply(id, True))
                except Exception:
                    await q_tx.put(MsgValidateTrackReply(id, False))

            case MsgDeleteTracks(ids):
                ctx = state.db_ctx()
                if ctx is not None:
                    await db.delete_tracks(ctx, ids)
                    await q_tx.put(MsgReloadTracks())
                    await update_collection()

            case MsgCreateYTAnonPlaylist(id, ids):
                url = await lib.yt_create_anon_playlist(ids)
                await q_tx.put(MsgYTAnonPlaylist(id, url))

            case MsgUpdateTrack(old, new):
                ctx = state.db_ctx()
                if ctx is not None:
                    await db.insert_track(ctx, new, ignore_if_exists=False)
                    if old.yt_id != new.yt_id:
                        await db.delete_track(ctx, old.yt_id)
                    await update_collection()

            case MsgCollectionContainsId(id, yt_id):
                ctx = state.db_ctx()
                if ctx is not None:
                    exists = await db.track_exists(ctx, yt_id)
                    await q_tx.put(MsgCollectionContainsIdResponse(id, exists))

            case MsgSearchCancelAll():
                if state.search_task is not None:
                    state.search_task.cancel()

            case MsgSearch(query, query_id, limit, kind):
                if state.search_task is not None:
                    state.search_task.cancel()

                if query:
                    match kind:
                        case "fuzzy":
                            state.search_task = asyncio.create_task(
                                search_fuzzy(query, query_id, limit)
                            )
                        case "by-artist":
                            state.search_task = asyncio.create_task(
                                search_by_artist(query, query_id, limit)
                            )
                        case "by-label":
                            state.search_task = asyncio.create_task(
                                search_by_label(query, query_id, limit)
                            )

            case MsgUpdateRating(yt_id, rating):
                ctx = state.db_ctx()
                if ctx is not None:
                    await db.update_track(ctx, yt_id, {"rating": rating})
                    track = await db.get_track(ctx, yt_id)
                    if track is not None:
                        await q_tx.put(MsgTrackUpdate(track))

            case MsgIncPlayCount(yt_id):
                ctx = state.db_ctx()
                if ctx is not None:
                    track = await db.get_track(ctx, yt_id)
                    if track is not None:
                        track.play_count += 1
                        track.last_played = datetime.now().date()
                        await db.insert_track(ctx, track, ignore_if_exists=False)
                        await q_tx.put(MsgTrackUpdate(track))

            case MsgGetRows(id, start_row, end_row, sort_model, filter_model):
                ctx = state.db_ctx()
                if ctx is not None:
                    n_rows = await db.count_rows(ctx, filter_model)
                    tracks = await db.get_rows_2(
                        ctx, start_row, end_row, sort_model, filter_model
                    )
                    await q_tx.put(MsgRows(id, tracks, n_rows))

            case MsgAddTrack(track, overwrite_existing):
                ctx = state.db_ctx()
                if ctx is not None:
                    await db.insert_track(
                        ctx, track, ignore_if_exists=not overwrite_existing
                    )
                    await q_tx.put(MsgTrackUpdate(track))
                    await update_collection()

            case MsgAddTracks(tracks, overwrite_existing):
                ctx = state.db_ctx()
                if ctx is not None:
                    await db.insert_tracks(
                        ctx, tracks, ignore_existing=not overwrite_existing
                    )
                    await q_tx.put(MsgReloadTracks())
                    await update_collection()

            case MsgGetTrackInfo(yt_id):
                ctx = state.db_ctx()
                if ctx is not None:
                    track = await db.get_track(ctx, yt_id)
                    if track is not None:
                        await q_tx.put(MsgTrackInfo(track))

            case MsgCreateCollection(path):
                path = Path(path)

                if path.exists():
                    await notify_client(
                        "Cannot create collection - file exists!", "error"
                    )
                    return

                path.parent.mkdir(parents=True, exist_ok=True)
                await db.init_db(db.Context(path))
                col = Collection(path, 0)
                state.collection = col
                await q_tx.put(MsgCollectionInfo(collection=state.collection))
                ev_state_change.set()

            case MsgSetCollection(id, path):
                is_valid = await db.validate_db_schema(path)
                if is_valid:
                    n = await db.count_rows(db.Context(Path(path)), {})
                    col = Collection(Path(path), n)
                    state.collection = col
                    await q_tx.put(MsgCollectionInfo(id, state.collection))
                else:
                    state.collection = None
                    completions = get_path_completions(path)
                    await q_tx.put(
                        MsgCollectionInfo(
                            id,
                            error_message="not a valid collection",
                            path_completions=completions,
                        )
                    )
                ev_state_change.set()

            case MsgGetPathCompletions(id, prefix):
                completions = get_path_completions(prefix)
                await q_tx.put(MsgPathCompletions(id, completions))

            case MsgImportFrom(path, keep_user_data):
                ctx = state.db_ctx()
                match is_file_or_url(path):
                    case "file":
                        if ctx is not None:
                            is_valid = await db.validate_db_schema(path)
                            if not is_valid:
                                await notify_client(
                                    f"Cannot import from invalid collection: {path}",
                                    "error",
                                )
                                return

                            await db.import_from_db(ctx, path, keep_user_data)
                            await update_collection()
                    case "url":
                        async with TemporaryDirectory() as tmp_dir:
                            db_path = Path(tmp_dir) / "temp-collection.db"
                            try:
                                await download_file(path, db_path)
                            except Exception as ex:
                                await notify_client(
                                    f"Downloading collection from {path} failed: {ex}",
                                    "error",
                                )
                                logger.exception(
                                    f"Failed to download collection from url: {path}"
                                )
                            else:
                                if ctx is not None:
                                    is_valid = await db.validate_db_schema(db_path)
                                    if not is_valid:
                                        await notify_client(
                                            f"Cannot import from invalid collection: {path}",
                                            "error",
                                        )
                                        return
                                    await db.import_from_db(
                                        ctx, str(db_path), keep_user_data
                                    )
                                    await update_collection()
                    case None:
                        await notify_client(
                            f"Import source {path} is neither file path nor url",
                            "error",
                        )

            case MsgCheckImportFrom(path):
                match is_file_or_url(path):
                    case "file":
                        is_valid = await db.validate_db_schema(path)
                        await q_tx.put(MsgImportFromValid(path, is_valid))
                    case "url":
                        async with TemporaryDirectory() as tmp_dir:
                            db_path = Path(tmp_dir) / "temp-collection.db"
                            try:
                                await download_file(path, db_path)
                            except Exception:
                                logger.exception(f"failed to download from {path}")
                                await q_tx.put(MsgImportFromValid(path, False))
                            else:
                                is_valid = await db.validate_db_schema(db_path)
                                await q_tx.put(MsgImportFromValid(path, is_valid))
                    case None:
                        await q_tx.put(MsgImportFromValid(path, False))

            case _:
                logger.warning(f"unhandled WS message: {msg}")

    async def handle_state_changes_loop():
        while True:
            # state_prev = copy.deepcopy(state)
            await ev_state_change.wait()

            ev_state_change.clear()
            await asyncio.sleep(0.1)  # debounce

    # send in chunks with a maximum delay (in seconds)
    async def send_loop(max_chunk_size: int, max_delay: float):
        buffer = []
        timeout = False
        while True:
            try:
                msg = await asyncio.wait_for(q_tx.get(), max_delay)
                buffer.append(asdict(msg))
                q_tx.task_done()
            except TimeoutError:
                timeout = True
            if len(buffer) >= max_chunk_size or (timeout and buffer):
                text = json.dumps(buffer, cls=JsonEnc)
                logger.info(f"sending WS message: {text[:500]}...")
                await websocket.send_text(text)
                buffer.clear()
                timeout = False

    async def recv_loop():
        while True:
            try:
                msg_text = await websocket.receive_text()
                logger.info(f"received WS message: {msg_text}")
                msg = decode_client_msg(msg_text)
            except WebSocketDisconnect:
                logger.info("websocket disconnected")
                break
            except ValidationError:
                logger.exception("failed to decode client message")
            except Exception:
                logger.exception("exception in receive loop")
            else:
                await q_rx.put(msg)

    async def handle_client_msg_loop():
        while True:
            msg = await q_rx.get()
            try:
                await handle_client_msg(msg)
            except Exception:
                logger.exception(f"failed to handle client message: {msg}")

    async def send_init_data():
        await q_tx.put(MsgBackendInfo(version=__version__))
        await q_tx.put(MsgDefaultCollection(default_collection))

    try:
        async with asyncio.TaskGroup() as tg:
            tg.create_task(send_init_data())
            tg.create_task(recv_loop())
            tg.create_task(handle_client_msg_loop())
            tg.create_task(handle_state_changes_loop())
            tg.create_task(send_loop(max_chunk_size=100, max_delay=0.1))
    except* Exception as e:
        logger.exception(f"WS connection {uid} exception in task group: {e.exceptions}")
        await websocket.close(1011)
    finally:
        del qs_tx[uid]
        del qs_rx[uid]
        logger.info(f"Closing WS connection {uid}")


@asynccontextmanager
async def lifespan(_):
    settings.home.mkdir(parents=True, exist_ok=True)
    setup_logger(settings.verbosity, settings.home)
    logger.info(f"Stereo settings: {settings}")
    yield


routes: list = [
    WebSocketRoute("/ws", websocket_endpoint),
]

if not settings.dev:
    routes.append(
        Mount("/", app=StaticFiles(html=True, packages=["stereo"]), name="static"),
    )

app = Starlette(routes=routes, lifespan=lifespan)
