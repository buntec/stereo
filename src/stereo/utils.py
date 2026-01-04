import os
from pathlib import Path
from typing import Literal
from urllib.parse import urlparse

import aiohttp


def get_path_completions(prefix: str) -> list[str]:
    path_obj = Path(prefix).expanduser()  # handle ~

    if prefix.endswith("/") or prefix == "~":
        parent = path_obj
        partial_name = ""
    else:
        parent = path_obj.parent
        partial_name = path_obj.name

    if not parent.exists() or not parent.is_dir():
        return []

    try:
        return [str(p) for p in parent.iterdir() if p.name.startswith(partial_name)]
    except PermissionError:
        return []


async def download_file(url: str, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    async with aiohttp.ClientSession() as session:
        async with session.get(url) as response:
            response.raise_for_status()
            with open(destination, mode="wb") as f:
                async for chunk in response.content.iter_chunked(1024):
                    f.write(chunk)


def is_file_or_url(path_or_url: str) -> Literal["file", "url"] | None:
    parsed = urlparse(path_or_url)

    if parsed.scheme in ("http", "https"):
        return "url"

    if os.path.isabs(path_or_url):
        return "file"

    return None
