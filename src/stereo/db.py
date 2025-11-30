import asyncio
import logging
import os
from dataclasses import dataclass
from pathlib import Path

import aiosqlite

from stereo.lib import Track
from stereo.message import (
    CombinedFilterModelItem,
    FilterModelItem,
    SortModelItem,
)

logger = logging.getLogger(__name__)


@dataclass
class Context:
    path: Path


async def init_db(ctx: Context) -> None:
    async with aiosqlite.connect(ctx.path) as db:
        await db.execute("""
            CREATE TABLE IF NOT EXISTS tracks (
                yt_id TEXT PRIMARY KEY,
                bp_id INTEGER,
                mb_id TEXT,
                title TEXT NOT NULL,
                mix_name TEXT,
                artists TEXT NOT NULL,
                release_date TEXT,
                label TEXT,
                album TEXT,
                length INTEGER,
                bpm INTEGER,
                genre TEXT,
                key TEXT,
                mood TEXT,
                rating INTEGER,
                play_count INTEGER DEFAULT 0,
                last_played TEXT
            )
        """)
        await db.commit()


async def insert_track(
    ctx: Context, track: Track, ignore_if_exists: bool = True
) -> None:
    logger.info(f"inserting track into db: {track}")

    data = track.to_db_row()

    fields = ", ".join(data.keys())
    placeholders = ", ".join([":" + k for k in data.keys()])

    on_conflict = "IGNORE" if ignore_if_exists else "REPLACE"

    async with aiosqlite.connect(ctx.path) as db:
        await db.execute(
            f"INSERT OR {on_conflict} INTO tracks ({fields}) VALUES ({placeholders})",
            data,
        )
        await db.commit()


async def insert_tracks(
    ctx: Context, tracks: list[Track], ignore_existing: bool = True
):
    logger.info(f"inserting tracks into db: {tracks}")
    params = []
    for track in tracks:
        params.append(track.to_db_row())

    if not params:
        return

    on_conflict = "IGNORE" if ignore_existing else "REPLACE"

    # Extract column names from the first dictionary to build the query
    columns = params[0].keys()
    fields = ", ".join(columns)
    placeholders = ", ".join([":" + k for k in columns])
    query = f"INSERT OR {on_conflict} INTO tracks ({fields}) VALUES ({placeholders})"

    async with aiosqlite.connect(ctx.path) as db:
        await db.executemany(query, params)
        await db.commit()


async def update_track(ctx: Context, yt_id: str, updates: dict) -> None:
    set_clause = ", ".join([f"{k} = :{k}" for k in updates.keys()])
    updates["yt_id"] = yt_id  # Ensure ID is in the params dict

    async with aiosqlite.connect(ctx.path) as db:
        await db.execute(
            f"UPDATE tracks SET {set_clause} WHERE yt_id = :yt_id", updates
        )
        await db.commit()


async def delete_track(ctx: Context, yt_id: str) -> None:
    logger.info(f"deleting track with yt_id: {yt_id}")
    async with aiosqlite.connect(ctx.path) as db:
        await db.execute("DELETE FROM tracks WHERE yt_id = ?", (yt_id,))
        await db.commit()


async def delete_tracks(ctx: Context, yt_ids: list[str]) -> int:
    logger.info(f"deleting tracks: {yt_ids}")
    if not yt_ids:
        return 0

    id_params = [(yt_id,) for yt_id in yt_ids]

    async with aiosqlite.connect(ctx.path) as db:
        await db.executemany("DELETE FROM tracks WHERE yt_id = ?", id_params)
        await db.commit()
        return db.total_changes


async def get_tracks(
    ctx: Context,
    filter_by: dict | None = None,
    sort_by: str | None = None,
    descending: bool = False,
    limit: int | None = None,
    offset: int | None = None,
) -> list[Track]:
    query = "SELECT * FROM tracks"
    params = []

    if filter_by:
        conditions = [f"{key} = ?" for key in filter_by.keys()]
        query += " WHERE " + " AND ".join(conditions)
        params = list(filter_by.values())

    if sort_by:
        direction = "DESC" if descending else "ASC"
        query += f" ORDER BY {sort_by} {direction}"

    if limit is not None:
        query += " LIMIT ?"
        params.append(limit)
        if offset is not None:
            query += " OFFSET ?"
            params.append(offset)

    async with aiosqlite.connect(ctx.path) as db:
        db.row_factory = aiosqlite.Row
        async with db.execute(query, params) as cursor:
            rows = await cursor.fetchall()

            results = []
            for row in rows:
                results.append(Track.from_db_row(dict(row)))
            return results


async def get_track(
    ctx: Context,
    yt_id: str,
) -> Track | None:
    query = "SELECT * FROM tracks WHERE yt_id = ?"

    async with aiosqlite.connect(ctx.path) as db:
        db.row_factory = aiosqlite.Row
        async with db.execute(query, (yt_id,)) as cursor:
            row = await cursor.fetchone()
            if row:
                return Track.from_db_row(dict(row))

            return None


async def get_random_track(ctx: Context, filter_by: dict | None = None) -> Track | None:
    query = "SELECT * FROM tracks"
    params = []

    if filter_by:
        conditions = [f"{key} = ?" for key in filter_by.keys()]
        query += " WHERE " + " AND ".join(conditions)
        params = list(filter_by.values())

    query += " ORDER BY RANDOM() LIMIT 1"

    async with aiosqlite.connect(ctx.path) as db:
        db.row_factory = aiosqlite.Row
        async with db.execute(query, params) as cursor:
            row = await cursor.fetchone()
            if row:
                return Track.from_db_row(dict(row))
            return None


async def get_total_track_count(ctx: Context) -> int:
    async with aiosqlite.connect(ctx.path) as db:
        async with db.execute("SELECT COUNT(*) FROM tracks") as cursor:
            row = await cursor.fetchone()
            return row[0] if row else 0


async def get_rows(
    ctx: Context, startRow: int, endRow: int, sort_model: list | None = None
) -> list[Track]:
    limit = endRow - startRow
    offset = startRow

    sort_by = None
    descending = False
    if sort_model:
        sort_by = sort_model[0]["colId"]
        descending = sort_model[0]["sort"] == "desc"

    tracks = await get_tracks(
        ctx, limit=limit, offset=offset, sort_by=sort_by, descending=descending
    )

    return tracks


async def get_rows_2(
    ctx: Context,
    startRow: int,
    endRow: int,
    sortModel: list[SortModelItem],
    filterModel: dict[str, FilterModelItem | CombinedFilterModelItem],
) -> list[Track]:
    query = "SELECT * FROM tracks"
    where_clauses = []
    params: list = []

    for field, item in filterModel.items():
        # TODO
        if isinstance(item, FilterModelItem):
            if item.filterType == "text" and item.type == "contains":
                where_clauses.append(f"{field} LIKE ?")
                params.append(f"%{item.filter}%")
            elif item.type == "equals":
                where_clauses.append(f"{field} = ?")
                params.append(item.filter)
            elif item.type == "blank":
                where_clauses.append(f"{field} IS NULL")
            elif item.type == "notBlank":
                where_clauses.append(f"{field} IS NOT NULL")

    if where_clauses:
        query += " WHERE " + " AND ".join(where_clauses)

    if sortModel:
        sort_parts: list[str] = []
        for sort_item in sortModel:
            direction = "ASC" if sort_item.sort == "asc" else "DESC"
            sort_parts.append(f"{sort_item.colId} {direction}")
        query += " ORDER BY " + ", ".join(sort_parts)

    query += " LIMIT ? OFFSET ?"
    params.extend([endRow - startRow, startRow])

    async with aiosqlite.connect(ctx.path) as db:
        db.row_factory = aiosqlite.Row
        async with db.execute(query, params) as cursor:
            rows = await cursor.fetchall()
            results = []
            for row in rows:
                results.append(Track.from_db_row(dict(row)))
            return results


async def count_rows(
    ctx: Context,
    filterModel: dict[str, FilterModelItem | CombinedFilterModelItem] = {},
) -> int:
    query = "SELECT COUNT(*) FROM tracks"
    where_clauses = []
    params: list = []

    for field, item in filterModel.items():
        # TODO
        if isinstance(item, FilterModelItem):
            if item.filterType == "text" and item.type == "contains":
                where_clauses.append(f"{field} LIKE ?")
                params.append(f"%{item.filter}%")
            elif item.type == "equals":
                where_clauses.append(f"{field} = ?")
                params.append(item.filter)

    if where_clauses:
        query += " WHERE " + " AND ".join(where_clauses)

    async with aiosqlite.connect(ctx.path) as db:
        async with db.execute(query, params) as cursor:
            row = await cursor.fetchone()
            return row[0] if row else 0


async def migrate_from_old_db(new_db_path: str, old_db_path: str):
    ctx = Context(Path(new_db_path))
    await init_db(ctx)
    async with aiosqlite.connect(ctx.path) as db:
        await db.execute("ATTACH DATABASE ? AS old_db", (old_db_path,))
        try:
            async with db.execute("PRAGMA main.table_info(tracks)") as cursor:
                new_cols = {row[1] for row in await cursor.fetchall()}

            async with db.execute("PRAGMA old_db.table_info(tracks)") as cursor:
                old_cols = {row[1] for row in await cursor.fetchall()}

            common_cols = list(new_cols.intersection(old_cols))
            if not common_cols:
                print("No matching columns found to migrate.")
                return

            col_names_str = ", ".join(common_cols)
            query = f"""
                INSERT OR IGNORE INTO main.tracks ({col_names_str})
                SELECT {col_names_str} FROM old_db.tracks
            """
            await db.execute(query)
            await db.commit()

            async with db.execute("SELECT changes()") as cursor:
                row = await cursor.fetchone()
                n = row[0] if row is not None else 0
                print(f"Migration complete. {n} tracks imported.")

        finally:
            await db.execute("DETACH DATABASE old_db")


async def import_from_db(
    ctx: Context,
    source_db: str,
    cols=[
        "yt_id",
        "title",
        "mix_name",
        "artists",
        "bp_id",
        "mb_id",
        "release_date",
        "label",
        "bpm",
        "key",
        "album",
        "genre",
        "mood",
    ],
):
    async with aiosqlite.connect(ctx.path) as db:
        await db.execute("ATTACH DATABASE ? AS source_db", (source_db,))
        try:
            async with db.execute("PRAGMA main.table_info(tracks)") as cursor:
                target_cols = {row[1] for row in await cursor.fetchall()}

            for col in cols:
                if col not in target_cols:
                    raise ValueError(f"{col} not in {target_cols}")

            async with db.execute("PRAGMA source_db.table_info(tracks)") as cursor:
                source_cols = {row[1] for row in await cursor.fetchall()}

            cols_to_import = list(set(cols).intersection(source_cols))

            if not cols_to_import:
                logger.warning("No columns found to import.")
                return

            col_names_str = ", ".join(cols_to_import)
            query = f"""
                INSERT OR IGNORE INTO main.tracks ({col_names_str})
                SELECT {col_names_str} FROM source_db.tracks
            """
            await db.execute(query)
            await db.commit()

            async with db.execute("SELECT changes()") as cursor:
                row = await cursor.fetchone()
                n = row[0] if row is not None else 0
                logger.info(f"Importing complete. {n} tracks imported.")

        finally:
            await db.execute("DETACH DATABASE source_db")


async def track_exists(ctx: Context, yt_id: str) -> bool:
    # 'SELECT 1' is a convention to check existence without fetching column data
    query = "SELECT 1 FROM tracks WHERE yt_id = ? LIMIT 1"

    async with aiosqlite.connect(ctx.path) as db:
        async with db.execute(query, (yt_id,)) as cursor:
            row = await cursor.fetchone()
            return row is not None


async def validate_db_schema(db_path: Path | str) -> bool:
    if not os.path.exists(db_path):
        return False

    expected_columns = set(Track.model_fields.keys())

    try:
        async with aiosqlite.connect(db_path) as db:
            # PRAGMA table_info returns: (id, name, type, notnull, default_value, pk)
            async with db.execute("PRAGMA table_info(tracks)") as cursor:
                rows = await cursor.fetchall()

                if not rows:
                    return False

                # Extract column names and primary key status
                found_columns = {row[1] for row in rows}
                pk_columns = {row[1] for row in rows if row[5] == 1}

                if not expected_columns.issubset(found_columns):
                    return False

                if "yt_id" not in pk_columns:
                    return False

                return True

    except aiosqlite.Error:
        # Not a valid sqlite database or file is corrupted
        return False


if __name__ == "__main__":

    async def run():
        await migrate_from_old_db("/path/to/new/stereo.db", "/path/to/old/stereo.db")

    asyncio.run(run())
