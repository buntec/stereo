import pytest
from aiohttp import ClientSession

import stereo.lib as lib


@pytest.mark.asyncio
async def test_bp_search():
    artist = "mika olson"
    title = "drift away"
    async with ClientSession() as session:
        tracks = await lib.bp_search_tracks(session, f"{artist} {title}")
        print(tracks[0])


@pytest.mark.asyncio
async def test_mb_search():
    title = "houdini"
    artist = "dua lipa"
    q = f"{title} AND artist:'{artist}'"
    async with ClientSession() as session:
        recs = await lib.mb_search_recording(q, session)
        print(recs[0])


@pytest.mark.asyncio
async def test_search_track_by_artist_and_title():
    title = "whispers"
    artist = "deco"
    async with ClientSession() as session:
        track = await lib.search_track_by_artist_and_title(
            title, artist, False, session
        )
        if track:
            print(track)
