import asyncio
import csv
import json
import logging
import time
from dataclasses import dataclass
from datetime import date, datetime
from pathlib import Path
from typing import Any, AsyncGenerator

import thefuzz.fuzz
import thefuzz.process
import yaml
from aiohttp import ClientSession
from bs4 import BeautifulSoup
from pydantic import BaseModel

logger = logging.getLogger(__name__)


@dataclass
class Collection:
    path: Path
    size: int


@dataclass
class BPLabel:
    display_name: str
    name: str
    id: int


@dataclass
class BPArtist:
    display_name: str
    name: str
    id: int
    image_uri: str


@dataclass
class BPTrack:
    artists: list[str]
    bpm: int | None
    key: str | None
    isrc: str | None
    label: str
    release_date: date
    track_id: int
    track_name: str
    mix_name: str  # e.g., "Original Mix"
    genre: list[str]


@dataclass
class YTVideo:
    title: str
    id: str
    channel: str


@dataclass
class MBRecording:
    title: str
    id: str
    artists: list[str]
    release_date: date | None


@dataclass
class CsvRow:
    song: str
    artist: str


class Track(BaseModel):
    yt_id: str  # primary key
    bp_id: int | None = None
    mb_id: str | None = None

    title: str
    mix_name: str | None = None
    artists: list[str]
    release_date: date | None = None
    label: str | None = None
    album: str | None = None
    length: int | None = None  # in seconds
    bpm: int | None = None
    genre: str | None = None
    key: str | None = None
    mood: str | None = None

    # user data
    rating: int | None = None
    play_count: int = 0
    last_played: date | None = None

    def _date_fields():
        return ["last_played", "release_date"]

    def from_bp_track(track: BPTrack, yt_id: str):
        return Track(
            yt_id=yt_id,
            title=track.track_name,
            mix_name=track.mix_name,
            artists=track.artists,
            bp_id=track.track_id,
            release_date=track.release_date,
            label=track.label,
            bpm=track.bpm,
            key=track.key,
            genre=", ".join(track.genre),
        )

    def to_db_row(self):
        data = self.model_dump()

        data["artists"] = json.dumps(data["artists"])

        for field in Track._date_fields():
            if data[field]:
                data[field] = data[field].isoformat()

        return data

    def from_db_row(row: dict[str, Any]):
        row["artists"] = json.loads(row["artists"])

        for field in Track._date_fields():
            if row[field]:
                row[field] = date.fromisoformat(row[field])

        return Track(**row)


def sort_key_track(track: Track):
    return f"{','.join(track.artists)} {track.release_date} {track.title}"


def dump_to_yaml(out_file: str, tracks: list[Track]):
    t1 = time.time()
    tracks_by_yt_id: dict[str, Track] = {t.yt_id: t for t in tracks}
    tracks = list(tracks_by_yt_id.values())
    tracks.sort(key=sort_key_track)

    with open(out_file, "w") as f:
        yaml.dump({"tracks": [t.model_dump() for t in tracks]}, f)
    t2 = time.time()
    print(f"saving to yaml took {t2 - t1} seconds")


def load_from_yaml(file: str) -> list[Track]:
    with open(file, "r") as f:
        t1 = time.time()
        doc = yaml.safe_load(f)
        tracks = []
        for t in doc["tracks"]:
            tracks.append(Track(**t))
        t2 = time.time()
        print(f"load took {t2 - t1} seconds")
        return tracks


def shorten_string(s: str, maxlen: int) -> str:
    if len(s) > maxlen:
        return f"{s[: maxlen - 3]}..."
    return s


async def bp_search_tracks(session: ClientSession, q: str) -> list[BPTrack]:
    logger.info(f"searching Beatport with query: {q}")
    async with session.get(
        "https://www.beatport.com/search/tracks",
        params={
            "q": q,
            # "order_by": "-release_date",
            "per_page": 1000,
            "page": 1,
        },
    ) as response:
        html = await response.text()
        logger.debug(f"Beatport response: {html}")
        soup = BeautifulSoup(html, "html.parser")
        script_tags = soup.find_all("script")
        tracks = []
        for tag in script_tags:
            if tag.get("id") == "__NEXT_DATA__":
                for el in tag.contents:
                    json_blob = json.loads(str(el))
                    for query in json_blob["props"]["pageProps"]["dehydratedState"][
                        "queries"
                    ]:
                        data = query.get("state", {}).get("data", {}).get("data", [])
                        for item in data:
                            artists = [x["artist_name"] for x in item["artists"]]
                            bpt = BPTrack(
                                artists,
                                item.get("bpm"),
                                item.get("key_name"),
                                item.get("isrc"),
                                label=item.get("label", {}).get("label_name"),
                                release_date=datetime.fromisoformat(
                                    item["release_date"]
                                ).date(),
                                track_id=item["track_id"],
                                track_name=item["track_name"].strip(),
                                mix_name=item["mix_name"].strip(),
                                genre=[i["genre_name"].strip() for i in item["genre"]],
                            )
                            tracks.append(bpt)
        logger.info(f"Beatport search results:\n{tracks}")
        return tracks


def yt_select_best_match(
    title: str, artist: str, candidates: list[YTVideo]
) -> YTVideo | None:
    assert candidates

    choices = [f"{r.title} - {artist}" for r in candidates]

    best = thefuzz.process.extractOne(
        title, choices, score_cutoff=50, scorer=thefuzz.fuzz.partial_token_sort_ratio
    )

    if best:
        i = choices.index(best[0])
        return candidates[i]

    return None


def mb_select_best_match(
    title: str, artist: str, candidates: list[MBRecording]
) -> MBRecording | None:
    assert candidates

    query = f"{title} - {artist}"
    choices = [f"{r.title} - {','.join(r.artists)}" for r in candidates]

    best = thefuzz.process.extractOne(
        query, choices, score_cutoff=50, scorer=thefuzz.fuzz.partial_token_sort_ratio
    )

    if best:
        i = choices.index(best[0])
        return candidates[i]

    return None


def bp_select_best_match(
    title: str, artist: str, candidates: list[BPTrack]
) -> BPTrack | None:
    assert candidates

    choices = [r.track_name for r in candidates]

    best_title_match = thefuzz.process.extractOne(title, choices, score_cutoff=50)

    i = None
    if best_title_match:
        i = choices.index(best_title_match[0])

    choices = [",".join(r.artists) for r in candidates]

    best_artist_match = thefuzz.process.extractOne(
        artist, choices, score_cutoff=50, scorer=thefuzz.fuzz.partial_token_sort_ratio
    )

    j = None
    if best_artist_match:
        j = choices.index(best_artist_match[0])

    if i is not None and j is not None:
        return candidates[i]

    return None


async def mb_search_recording(query: str, session: ClientSession) -> list[MBRecording]:
    """example query: "love mythology AND artist:'henry saiz'"""

    logger.info(f"searching MusicBrainz with query: {query}")

    async with session.get(
        "http://musicbrainz.org/ws/2/recording",
        params={"fmt": "json", "query": query},
    ) as response:
        data = await response.json()
        logger.debug(f"MusicBrainz response: {data}")
        # with open("dump.json", "w") as f:
        #     json.dump(data, f)
        recs = []
        for rec in data.get("recordings", []):
            id = rec.get("id")
            title = rec.get("title")
            release_date = None
            if "first-release-date" in rec:
                try:
                    release_date = date.fromisoformat(rec["first-release-date"])
                except ValueError:
                    pass
            artists = []
            for a in rec.get("artist-credit", []):
                name = a.get("name")
                artists.append(name)
            recs.append(MBRecording(title, id, artists, release_date))
        logger.info(f"MusicBrainz search results:\n{recs}")
        return recs


async def retry(f, n: int, label: str):
    result = None
    backoff = 1.0
    for i in range(n):
        try:
            result = await f()
            return result
        except Exception:
            logger.exception(f"{label} failed: retry in {backoff} seconds...")
            await asyncio.sleep(backoff)
            backoff *= 2


def read_pl_from_csv(filepath: str) -> list[CsvRow]:
    with open(filepath, newline="") as csvfile:
        reader = csv.reader(csvfile, delimiter=",")
        next(reader)  # skip header row
        rows = []
        for row in reader:
            song = row[1]
            artist = row[2]
            rows.append(CsvRow(song, artist))
        return rows


async def bp_search_labels(
    session: ClientSession, name: str, limit: int
) -> list[BPLabel]:
    async with session.get(
        "https://www.beatport.com/search/labels",
        params={"q": name},
    ) as response:
        html = await response.text()
        soup = BeautifulSoup(html, "html.parser")
        script_tags = soup.find_all("script")
        labels = []
        for tag in script_tags:
            if tag.get("id") == "__NEXT_DATA__":
                for el in tag.contents:
                    json_blob = json.loads(str(el))
                    for query in json_blob["props"]["pageProps"]["dehydratedState"][
                        "queries"
                    ]:
                        data = query.get("state", {}).get("data", {}).get("data", [])
                        for item in data:
                            labels.append(
                                BPLabel(
                                    item["label_name"].strip(),
                                    item["label_name"].strip(),
                                    item["label_id"],
                                )
                            )
                            if len(labels) >= limit:
                                return labels
        return labels


async def bp_search_artist(
    session: ClientSession, name: str, limit: int
) -> list[BPArtist]:
    async with session.get(
        "https://www.beatport.com/search/artists",
        params={"q": name},
    ) as response:
        html = await response.text()
        soup = BeautifulSoup(html, "html.parser")
        script_tags = soup.find_all("script")
        artists = []
        for tag in script_tags:
            if tag.get("id") == "__NEXT_DATA__":
                for el in tag.contents:
                    json_blob = json.loads(str(el))
                    for query in json_blob["props"]["pageProps"]["dehydratedState"][
                        "queries"
                    ]:
                        data = query.get("state", {}).get("data", {}).get("data", [])
                        for item in data:
                            artists.append(
                                BPArtist(
                                    item["artist_name"].strip(),
                                    item["artist_name"].strip(),
                                    item["artist_id"],
                                    item["artist_image_uri"],
                                )
                            )
                            if len(artists) >= limit:
                                return artists
        return artists


async def bp_get_label_releases(
    session: ClientSession,
    label: BPLabel,
    start_date: date | None = None,
    end_date: date | None = None,
) -> list[BPTrack]:
    params: dict = {
        "page": 1,
        "per_page": 1000,
    }

    if start_date is not None:
        end_date = end_date if end_date is not None else date.today()
        params["publish_date"] = f"{start_date.isoformat()}:{end_date.isoformat()}"

    elif end_date is not None:
        start_date = date(1990, 1, 1)
        params["publish_date"] = f"{start_date.isoformat()}:{end_date.isoformat()}"

    async with session.get(
        f"https://www.beatport.com/label/{label.name}/{label.id}/tracks",
        params=params,
    ) as response:
        html = await response.text()
        logger.debug(f"Beatport label release search response: {html}")
        bp_tracks = []
        soup = BeautifulSoup(html, "html.parser")
        script_tags = soup.find_all("script")
        for tag in script_tags:
            if tag.get("id") == "__NEXT_DATA__":
                for el in tag.contents:
                    json_blob = json.loads(str(el))
                    for query in json_blob["props"]["pageProps"]["dehydratedState"][
                        "queries"
                    ]:
                        if "tracks" in query["queryKey"]:
                            tracks = (
                                query.get("state", {})
                                .get("data", {})
                                .get("results", [])
                            )
                            for track in tracks:
                                artists = [a["name"].strip() for a in track["artists"]]
                                track_name = track["name"].strip()
                                mix_name = track["mix_name"].strip()
                                isrc = track["isrc"].strip()
                                key = track["key"]["name"].strip()
                                release_date = date.fromisoformat(
                                    track["new_release_date"]
                                )
                                bpm = track["bpm"]
                                genre = track["genre"]["name"].strip()
                                track_id = track["id"]
                                label = track["release"]["label"]["name"].strip()
                                bp_tracks.append(
                                    BPTrack(
                                        artists,
                                        bpm,
                                        key,
                                        isrc,
                                        label,
                                        release_date,
                                        track_id,
                                        track_name,
                                        mix_name,
                                        [genre],
                                    )
                                )
        return bp_tracks


async def bp_get_artist_releases(
    session: ClientSession,
    artist: BPArtist,
    start_date: date | None = None,
    end_date: date | None = None,
) -> list[BPTrack]:
    params: dict = {
        "page": 1,
        "per_page": 1000,
    }

    if start_date is not None:
        end_date = end_date if end_date is not None else date.today()
        params["publish_date"] = f"{start_date.isoformat()}:{end_date.isoformat()}"

    elif end_date is not None:
        start_date = date(1990, 1, 1)
        params["publish_date"] = f"{start_date.isoformat()}:{end_date.isoformat()}"

    async with session.get(
        f"https://www.beatport.com/artist/{artist.name}/{artist.id}/tracks",
        params=params,
    ) as response:
        html = await response.text()
        logger.debug(f"Beatport label release search response: {html}")
        bp_tracks = []
        soup = BeautifulSoup(html, "html.parser")
        script_tags = soup.find_all("script")
        for tag in script_tags:
            if tag.get("id") == "__NEXT_DATA__":
                for el in tag.contents:
                    json_blob = json.loads(str(el))
                    for query in json_blob["props"]["pageProps"]["dehydratedState"][
                        "queries"
                    ]:
                        if "tracks" in query["queryKey"]:
                            tracks = (
                                query.get("state", {})
                                .get("data", {})
                                .get("results", [])
                            )
                            for track in tracks:
                                artists = [a["name"].strip() for a in track["artists"]]
                                track_name = track["name"].strip()
                                mix_name = track["mix_name"].strip()
                                isrc = track["isrc"].strip()
                                key = track["key"]["name"].strip()
                                release_date = date.fromisoformat(
                                    track["new_release_date"]
                                )
                                bpm = track["bpm"]
                                genre = track["genre"]["name"].strip()
                                track_id = track["id"]
                                label = track["release"]["label"]["name"].strip()
                                bp_tracks.append(
                                    BPTrack(
                                        artists,
                                        bpm,
                                        key,
                                        isrc,
                                        label,
                                        release_date,
                                        track_id,
                                        track_name,
                                        mix_name,
                                        [genre],
                                    )
                                )
        return bp_tracks


async def bp_search_label_releases_fallback(
    session: ClientSession,
    label: BPLabel,
    start_date: date,
    end_date: date = date.today(),
) -> list[BPTrack]:
    async with session.get(
        f"https://www.beatport.com/label/{label.name}/{label.id}/tracks",
        params={
            "page": 1,
            "per_page": 1000,
            "publish_date": f"{start_date.isoformat()}:{end_date.isoformat()}",
        },
    ) as response:
        html = await response.text()
        soup = BeautifulSoup(html, "html.parser")
        elements = soup.select(".row.tracks-table")
        tracks = []
        for element in elements:
            title_tag = element.select("div.title a")[0]
            track_name = str(title_tag.attrs["title"])
            track_id = int(str(title_tag.attrs["href"]).split("/")[-1])

            title_strings = list(title_tag.stripped_strings)
            # track_name = title_strings[0]
            mix_name = title_strings[1]

            artists = []

            artist_tags = element.select("div.title a")[1:]
            for tag in artist_tags:
                artists.append(str(tag.attrs["title"]))

            label_tag = element.select("div.label a")[0]
            label0 = str(label_tag.attrs["title"])

            # tracks = element.find_all(href=re.compile("^/track"))
            # for track in tracks:
            #     print(track["href"])

            genre_tag = element.select("div.bpm > a")[0]
            genre = str(genre_tag.attrs["title"])

            bpm_tag = element.select("div.bpm > div")[0]
            bpm = int(str(bpm_tag.contents[0]))
            key = str(bpm_tag.contents[4]).strip("- ")

            date_tag = element.select("div.date")[0]
            release_date = date.fromisoformat(date_tag.text)
            track = BPTrack(
                artists,
                bpm,
                key,
                None,
                label0,
                release_date,
                track_id,
                track_name,
                mix_name,
                [genre],
            )
            tracks.append(track)
        return tracks


async def yt_search_simple(
    session: ClientSession, query: str, limit: int
) -> list[YTVideo]:
    HEADERS = {
        "User-Agent": "Mozilla/5.0",
        "Accept-Language": "en-US,en;q=0.9",
        "Content-Type": "application/json",
    }

    CLIENTS = [
        ("WEB", "2.20240201.00.00"),
        ("MWEB", "2.20240201.00.00"),
        ("ANDROID", "19.09.37"),
    ]

    def _get_text(runs):
        return runs[0]["text"] if runs else ""

    def _extract_results(data: Any, limit: int) -> list[YTVideo]:
        results = []

        sections = (
            data.get("contents", {})
            .get("twoColumnSearchResultsRenderer", {})
            .get("primaryContents", {})
            .get("sectionListRenderer", {})
            .get("contents", [])
        )

        for section in sections:
            items = section.get("itemSectionRenderer", {}).get("contents", [])
            for item in items:
                video = item.get("videoRenderer")
                if not video:
                    continue

                video_id = video.get("videoId")
                title = _get_text(video.get("title", {}).get("runs", []))

                channel = _get_text(
                    video.get("ownerText", {}).get("runs", [])
                ) or _get_text(video.get("longBylineText", {}).get("runs", []))

                results.append(YTVideo(title, video_id, channel))

                if len(results) >= limit:
                    return results

        return results

    # cycle through clients until first success
    for name, version in CLIENTS:
        try:
            response = await session.post(
                "https://www.youtube.com/youtubei/v1/search",
                headers=HEADERS,
                params={"key": "AIzaSyDummyKey"},
                json={
                    "context": {
                        "client": {
                            "clientName": name,
                            "clientVersion": version,
                        }
                    },
                    "query": query,
                },
            )

            data = await response.json()

            # with open("dump.json", "w") as f:
            #     json.dump(data, f)

            results = _extract_results(data, limit)
            if results:
                return results

        except Exception:
            pass

    return []


# not super useful as it doesn't include music meta data
async def yt_get_metadata(session: ClientSession, video_id: str):
    HEADERS = {
        "User-Agent": "Mozilla/5.0",
        "Accept-Language": "en-US,en;q=0.9",
        "Content-Type": "application/json",
    }

    CLIENTS = [
        ("WEB", "2.20240201.00.00"),
        ("ANDROID", "19.09.37"),
        ("MWEB", "2.20240201.00.00"),
    ]
    # cycle through clients until first success
    for name, version in CLIENTS:
        try:
            response = await session.post(
                "https://www.youtube.com/youtubei/v1/player",
                headers=HEADERS,
                params={"key": "AIzaSyDummyKey"},
                json={
                    "context": {
                        "client": {
                            "clientName": name,
                            "clientVersion": version,
                        }
                    },
                    "videoId": video_id,
                },
            )

            data = await response.json()

            with open("dump.json", "w") as f:
                json.dump(data, f)

            print(data)

        except Exception:
            pass

    return []


async def get_label_releases(
    session: ClientSession, label_name: str, start_date: date | None = None
) -> AsyncGenerator[Track]:
    bp_labels = await bp_search_labels(session, label_name, limit=1)
    if bp_labels:
        tracks = await bp_get_label_releases(session, bp_labels[0], start_date)
        for track in tracks:
            q = f"{track.track_name} {', '.join(track.artists)} {track.label}"
            yt_vids = await yt_search_simple(session, q, 1)
            yield Track.from_bp_track(track, yt_vids[0].id)


async def get_artist_releases(
    session: ClientSession, artist_name: str, start_date: date | None = None
) -> AsyncGenerator[Track]:
    bp_artists = await bp_search_artist(session, artist_name, limit=1)
    if bp_artists:
        tracks = await bp_get_artist_releases(session, bp_artists[0], start_date)
        for track in tracks:
            q = f"{track.track_name} {', '.join(track.artists)} {track.label}"
            yt_vids = await yt_search_simple(session, q, 1)
            yield Track.from_bp_track(track, yt_vids[0].id)


async def search_fuzzy(session: ClientSession, query: str) -> AsyncGenerator[Track]:
    bp_tracks = await bp_search_tracks(session, query)
    for track in bp_tracks:
        q = f"{track.track_name} {', '.join(track.artists)} {track.label}"
        yt_vids = await yt_search_simple(session, q, 1)
        if yt_vids:
            yield Track.from_bp_track(track, yt_vids[0].id)


async def get_final_url(url):
    async with ClientSession() as session:
        async with session.get(url, allow_redirects=True) as response:
            return response.url


async def yt_create_anon_playlist(video_ids: list[str]) -> str:
    base_url = "http://www.youtube.com/watch_videos?video_ids="
    url = f"{base_url}{','.join(video_ids)}"
    final_url = await get_final_url(url)
    list = final_url.query.get("list")
    return f"https://music.youtube.com/watch?list={list}"


if __name__ == "__main__":

    async def test_yt_search():
        async with ClientSession() as session:
            vids = await yt_search_simple(session, "walls mosoo deco", 3)
            print(vids)

    async def test_yt_get_metadata():
        async with ClientSession() as session:
            md = await yt_get_metadata(session, "ieLNrlLUhWo")
            print(md)

    async def test_get_label_releases():
        async with ClientSession() as session:
            async for track in get_label_releases(
                session, "Magnifik Music", date(2025, 1, 1)
            ):
                print(track)

    async def test_get_artist_releases():
        async with ClientSession() as session:
            async for track in get_artist_releases(
                session, "kadosh ofc", date(2025, 1, 1)
            ):
                print(track)

    async def test_mb_search():
        async with ClientSession() as session:
            tracks = await mb_search_recording(
                'recording:"houdini" AND artist:"dua lipa"', session
            )
            print(tracks)

    async def test_bp_search():
        async with ClientSession() as session:
            tracks = await bp_search_tracks(session, "kadosh")
            print(tracks)

    async def test_bp_search_artist():
        async with ClientSession() as session:
            artist = await bp_search_artist(session, "kadosh", 10)
            print(artist[:5])

    async def test_search_fuzzy():
        async with ClientSession() as session:
            query = "auguxt what that means"
            # query = "henry saiz love mythology"
            i = 0
            async for track in search_fuzzy(session, query):
                i += 1
                print(track)
                if i >= 5:
                    break

    async def test_yt_anon_playlist():
        ids = ["nP60jajfMdw", "0h6VHeysvh4", "tDDsbq8PqBM"]
        url = await yt_create_anon_playlist(ids)
        print(url)

    # asyncio.run(test_yt_search())
    # asyncio.run(test_get_label_releases())
    # asyncio.run(test_get_artist_releases())
    asyncio.run(test_search_fuzzy())
    # asyncio.run(test_bp_search())
    # asyncio.run(test_bp_search_artist())
    # asyncio.run(test_yt_get_metadata())
    # asyncio.run(test_mb_search())
    # asyncio.run(test_yt_anon_playlist())
