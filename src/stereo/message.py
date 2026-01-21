from dataclasses import dataclass, field
from typing import Any, Literal

from stereo.lib import Collection, Track


@dataclass
class MsgCollectionInfo:
    id: int | None = None
    collection: Collection | None = None
    error_message: str | None = None
    path_completions: list[str] | None = None
    type: Literal["collection-info"] = "collection-info"


@dataclass
class MsgDefaultCollection:
    collection: Collection
    type: Literal["default-collection"] = "default-collection"


@dataclass
class MsgCreateYTAnonPlaylist:
    id: int
    yt_ids: list[str]
    type: Literal["create-yt-anon-playlist"] = "create-yt-anon-playlist"


@dataclass
class MsgYTAnonPlaylist:
    id: int
    url: str
    type: Literal["yt-anon-playlist"] = "yt-anon-playlist"


@dataclass
class MsgUpdateTrack:
    old: Track
    new: Track
    type: Literal["update-track"] = "update-track"


@dataclass
class MsgHeartbeat:
    timestamp: int
    type: Literal["heartbeat"] = "heartbeat"


@dataclass
class MsgImportFrom:
    path: str
    keep_user_data: bool
    type: Literal["import-from"] = "import-from"


@dataclass
class MsgCheckImportFrom:
    path: str
    type: Literal["check-import-from"] = "check-import-from"


@dataclass
class MsgImportFromValid:
    path: str
    is_valid: bool
    type: Literal["import-from-valid"] = "import-from-valid"


@dataclass
class MsgBackendInfo:
    version: str
    type: Literal["backend-info"] = "backend-info"


@dataclass
class MsgCollectionContainsId:
    id: int  # request id
    yt_id: str
    type: Literal["collection-contains-id"] = "collection-contains-id"


@dataclass
class MsgCollectionContainsIdResponse:
    id: int  # request id
    contains_id: bool
    type: Literal["collection-contains-id-response"] = "collection-contains-id-response"


@dataclass
class MsgDeleteTracks:
    ids: list[str]
    type: Literal["delete-tracks"] = "delete-tracks"


@dataclass
class MsgExportTracksToCollection:
    tracks: list[Track]
    collection: str
    type: Literal["export-tracks-to-collection"] = "export-tracks-to-collection"


@dataclass
class MsgValidateTrack:
    id: int  # request id
    track: dict[str, Any]
    type: Literal["validate-track"] = "validate-track"


@dataclass
class MsgValidateTrackReply:
    id: int  # request id
    is_valid: bool
    type: Literal["validate-track-reply"] = "validate-track-reply"


@dataclass
class MsgSearch:
    query: str
    query_id: int
    limit: int
    kind: Literal["fuzzy", "by-artist", "by-label"] = "fuzzy"
    type: Literal["search"] = "search"


@dataclass
class MsgSearchCancelAll:
    type: Literal["search-cancel-all"] = "search-cancel-all"


@dataclass
class MsgSearchResults:
    query_id: int
    tracks: list[Track]
    type: Literal["search-results"] = "search-results"


@dataclass
class MsgSearchResult:
    query_id: int
    track: Track
    type: Literal["search-result"] = "search-result"


@dataclass
class MsgSearchComplete:
    query_id: int
    type: Literal["search-complete"] = "search-complete"


@dataclass
class MsgSetCollection:
    id: int  # request id
    path: str
    type: Literal["set-collection"] = "set-collection"


@dataclass
class MsgCreateCollection:
    path: str
    type: Literal["create-collection"] = "create-collection"


@dataclass
class MsgAddTrack:
    track: Track
    overwrite_existing: bool = False
    type: Literal["add-track"] = "add-track"


@dataclass
class MsgAddTracks:
    tracks: list[Track]
    overwrite_existing: bool = False
    type: Literal["add-tracks"] = "add-tracks"


@dataclass
class MsgGetPathCompletions:
    id: int  # request id
    path_prefix: str
    type: Literal["get-path-completions"] = "get-path-completions"


@dataclass
class MsgPathCompletions:
    id: int  # request id
    paths: list[str]
    type: Literal["path-completions"] = "path-completions"


@dataclass
class MsgRows:
    id: int  # request id
    rows: list[Track]
    last_row: int | None = None
    type: Literal["rows"] = "rows"


@dataclass
class MsgTrackUpdate:
    track: Track
    type: Literal["track-update"] = "track-update"


@dataclass
class MsgReloadTracks:
    type: Literal["reload-tracks"] = "reload-tracks"


@dataclass
class MsgUpdateRating:
    yt_id: str
    rating: int | None
    type: Literal["update-rating"] = "update-rating"


@dataclass
class MsgIncPlayCount:
    yt_id: str
    type: Literal["inc-play-count"] = "inc-play-count"


@dataclass
class SortModelItem:
    colId: str
    sort: str
    type: str | None


@dataclass
class FilterModelItem:
    filterType: str
    type: str
    filter: Any = None
    filterTo: Any = None  # Used for 'between' ranges
    dateFrom: str | None = None  # Used for date filters
    dateTo: str | None = None


@dataclass
class CombinedFilterModelItem:
    filterType: str
    operator: str  # 'AND' or 'OR'
    conditions: list[FilterModelItem]


@dataclass
class MsgGetRows:
    id: int
    startRow: int
    endRow: int
    sortModel: list[SortModelItem] = field(default_factory=list)
    filterModel: dict[str, FilterModelItem | CombinedFilterModelItem] = field(
        default_factory=dict
    )
    type: Literal["get-rows"] = "get-rows"


@dataclass
class MsgGetRowIndex:
    id: int
    yt_id: str
    sortModel: list[SortModelItem] = field(default_factory=list)
    filterModel: dict[str, FilterModelItem | CombinedFilterModelItem] = field(
        default_factory=dict
    )
    type: Literal["get-row-index"] = "get-row-index"


@dataclass
class MsgRowIndex:
    id: int
    index: int
    type: Literal["row-index"] = "row-index"


@dataclass
class MsgGetTrackInfo:
    yt_id: str
    type: Literal["get-track-info"] = "get-track-info"


@dataclass
class MsgTrackInfo:
    track: Track
    type: Literal["track-info"] = "track-info"


@dataclass
class MsgNotification:
    message: str
    kind: Literal["info", "warn", "warning", "error"]
    type: Literal["notification"] = "notification"


type MsgServer = (
    MsgHeartbeat
    | MsgTrackUpdate
    | MsgReloadTracks
    | MsgRows
    | MsgBackendInfo
    | MsgTrackInfo
    | MsgCollectionInfo
    | MsgDefaultCollection
    | MsgPathCompletions
    | MsgSearchResult
    | MsgSearchResults
    | MsgSearchComplete
    | MsgCollectionContainsIdResponse
    | MsgNotification
    | MsgImportFromValid
    | MsgYTAnonPlaylist
    | MsgValidateTrackReply
    | MsgRowIndex
)

type MsgClient = (
    MsgHeartbeat
    | MsgDeleteTracks
    | MsgUpdateRating
    | MsgIncPlayCount
    | MsgGetRows
    | MsgAddTrack
    | MsgAddTracks
    | MsgGetTrackInfo
    | MsgSetCollection
    | MsgCreateCollection
    | MsgGetPathCompletions
    | MsgSearch
    | MsgSearchCancelAll
    | MsgCollectionContainsId
    | MsgCheckImportFrom
    | MsgImportFrom
    | MsgCreateYTAnonPlaylist
    | MsgUpdateTrack
    | MsgValidateTrack
    | MsgExportTracksToCollection
    | MsgGetRowIndex
)
