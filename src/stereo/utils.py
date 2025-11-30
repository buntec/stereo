from pathlib import Path


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
