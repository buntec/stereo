import { Toolbar } from "@astryxdesign/core/Toolbar";
import { Selector } from "@astryxdesign/core/Selector";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { TextInput } from "@astryxdesign/core/TextInput";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Spinner } from "@astryxdesign/core/Spinner";
import { Tooltip } from "@astryxdesign/core/Tooltip";
import { HStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { MagnifyingGlassIcon } from "@radix-ui/react-icons";

import { type SearchBoxKind, type TrackSource } from "./Types.tsx";

export interface ISearchBoxProps {
  value: string;
  setValue: (value: string) => void;
  onSearch: () => void;
  searchKind: SearchBoxKind;
  setSearchKind: (value: SearchBoxKind) => void;
  searchLimit: number;
  setSearchLimit: (value: number) => void;
  searchSources: TrackSource[];
  setSearchSources: (value: TrackSource[]) => void;
  newReleasesDays: number;
  setNewReleasesDays: (days: number) => void;
  searchBusy: boolean;
  errorMessage?: string;
}

const SEARCH_KIND_OPTIONS = [
  { value: "fuzzy", label: "Fuzzy" },
  { value: "by-label", label: "Label" },
  { value: "by-artist", label: "Artist" },
  { value: "new-releases", label: "Latest releases" },
];

const LIMIT_OPTIONS = ["10", "25", "50", "100", "500", "1000"];

const ALL_SEARCH_SOURCES: { value: TrackSource; label: string }[] = [
  { value: "beatport", label: "Beatport" },
  { value: "soundcloud", label: "SoundCloud" },
  { value: "volumo", label: "Volumo" },
  { value: "traxsource", label: "Traxsource" },
  { value: "spotify", label: "Spotify" },
];

const DAY_OPTIONS = [1, 7, 14, 30, 60, 90, 180, 365];

function toSearchKind(s: string): SearchBoxKind {
  switch (s) {
    case "fuzzy":
      return "fuzzy";
    case "by-label":
      return "by-label";
    case "by-artist":
      return "by-artist";
    case "new-releases":
      return "new-releases";
  }

  throw new Error(`cannot convert ${s} to search kind!`);
}

function SearchBox({
  value,
  setValue,
  onSearch,
  searchKind,
  setSearchKind,
  searchLimit,
  setSearchLimit,
  searchSources,
  setSearchSources,
  newReleasesDays,
  setNewReleasesDays,
  searchBusy,
  errorMessage,
}: ISearchBoxProps) {
  const isNewReleases = searchKind === "new-releases";

  const kindSelect = (
    <Selector
      label="Search kind"
      isLabelHidden
      options={SEARCH_KIND_OPTIONS}
      value={searchKind}
      onChange={(v) => setSearchKind(toSearchKind(v))}
    />
  );

  const limitSelect = (
    <Selector
      label="Result limit"
      isLabelHidden
      options={LIMIT_OPTIONS}
      value={`${searchLimit}`}
      onChange={(v) => setSearchLimit(parseInt(v))}
    />
  );

  const sourcesMenu = (
    <MultiSelector
      label="Search sources"
      isLabelHidden
      placeholder="Sources"
      triggerDisplay="count"
      options={ALL_SEARCH_SOURCES}
      value={searchSources}
      onChange={(v) => setSearchSources(v as TrackSource[])}
    />
  );

  return (
    <Toolbar
      className="search-box"
      label="Search"
      size="sm"
      startContent={
        <>
          <Tooltip content="Search by label, by artist, fuzzy, or latest releases">
            {kindSelect}
          </Tooltip>
          {isNewReleases ? (
            <HStack
              className="search-box-text-field"
              gap={2}
              vAlign="center"
              paddingInline={2}
            >
              {searchBusy ? <Spinner size="sm" label="Searching" /> : null}
              <Selector
                label="Days"
                isLabelHidden
                options={DAY_OPTIONS.map((d) => ({
                  value: `${d}`,
                  label: `Last ${d} day${d === 1 ? "" : "s"}`,
                }))}
                value={`${newReleasesDays}`}
                onChange={(v) => setNewReleasesDays(parseInt(v))}
              />
              <Tooltip content="Limit number of search results">
                {limitSelect}
              </Tooltip>
              <Tooltip content="Search sources">{sourcesMenu}</Tooltip>
            </HStack>
          ) : (
            <>
              <TextInput
                className="search-box-text-field"
                label="Search"
                isLabelHidden
                startIcon="search"
                hasClear
                isLoading={searchBusy}
                value={value}
                onChange={setValue}
                onKeyDown={(ev) => {
                  if (ev.key === "Enter") onSearch();
                }}
              />
              <Tooltip content="Limit number of search results">
                {limitSelect}
              </Tooltip>
              <Tooltip content="Search sources">{sourcesMenu}</Tooltip>
            </>
          )}
        </>
      }
      endContent={
        <>
          <IconButton
            variant="primary"
            label="Search"
            tooltip="Search"
            icon={<MagnifyingGlassIcon />}
            isDisabled={!isNewReleases && value.trim().length === 0}
            onClick={onSearch}
          />
          {errorMessage ? (
            <Text style={{ color: "var(--color-error)" }}>{errorMessage}</Text>
          ) : null}
        </>
      }
    />
  );
}

export default SearchBox;
