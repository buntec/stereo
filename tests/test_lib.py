"""Integration tests for lib.py — hit real external APIs."""

import pytest
from aiohttp import ClientSession
from datetime import date

from stereo.lib import (
    BeatportSession,
    bp_search_artist,
    bp_search_tracks,
    get_artist_releases,
    get_label_releases,
    mb_search_recording,
    search_fuzzy,
    txs_search,
    yt_create_anon_playlist,
    yt_search_simple,
)


@pytest.mark.asyncio
async def test_yt_search():
    async with ClientSession() as session:
        vids = await yt_search_simple(session, "walls mosoo deco", 3)
    assert len(vids) > 0
    assert vids[0].id
    assert vids[0].title


@pytest.mark.asyncio
async def test_bp_search_tracks():
    async with BeatportSession(impersonate="chrome124") as session:
        tracks = await bp_search_tracks(session, "kadosh")
    assert len(tracks) > 0
    assert tracks[0].track_name
    assert tracks[0].artists


@pytest.mark.asyncio
async def test_bp_search_artist():
    async with BeatportSession(impersonate="chrome124") as session:
        artists = await bp_search_artist(session, "kadosh", 10)
    assert len(artists) > 0
    assert artists[0].display_name


@pytest.mark.asyncio
async def test_get_label_releases():
    async with ClientSession() as session:
        tracks = []
        async for track in get_label_releases(session, "Magnifik Music", date(2025, 1, 1)):
            tracks.append(track)
            if len(tracks) >= 3:
                break
    assert len(tracks) > 0
    assert tracks[0].yt_id
    assert tracks[0].label == "Magnifik Music"


@pytest.mark.asyncio
async def test_get_artist_releases():
    async with ClientSession() as session:
        tracks = []
        async for track in get_artist_releases(session, "kadosh ofc", date(2025, 1, 1)):
            tracks.append(track)
            if len(tracks) >= 3:
                break
    assert len(tracks) > 0
    assert tracks[0].yt_id
    assert tracks[0].artists


@pytest.mark.asyncio
async def test_search_fuzzy():
    async with ClientSession() as session:
        tracks = []
        async for track in search_fuzzy(session, "auguxt what that means"):
            tracks.append(track)
            if len(tracks) >= 3:
                break
    assert len(tracks) > 0
    assert tracks[0].yt_id


@pytest.mark.asyncio
async def test_mb_search():
    async with ClientSession() as session:
        recordings = await mb_search_recording(
            'recording:"houdini" AND artist:"dua lipa"', session
        )
    assert len(recordings) > 0
    assert recordings[0].title
    assert recordings[0].artists


@pytest.mark.asyncio
async def test_yt_anon_playlist():
    ids = ["nP60jajfMdw", "0h6VHeysvh4", "tDDsbq8PqBM"]
    url = await yt_create_anon_playlist(ids)
    assert url.startswith("https://music.youtube.com/watch")
    assert "list=" in url


@pytest.mark.asyncio
async def test_txs_search():
    tracks = await txs_search("juno ride with me")
    assert len(tracks) > 0
    assert tracks[0].track_name
    assert tracks[0].artists
