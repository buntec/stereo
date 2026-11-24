import { Selector } from "@astryxdesign/core/Selector";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
} from "@astryxdesign/core/DropdownMenu";
import { InputGroup } from "@astryxdesign/core/InputGroup";
import { TextInput } from "@astryxdesign/core/TextInput";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Divider } from "@astryxdesign/core/Divider";
import { HStack, StackItem } from "@astryxdesign/core/Stack";
import {
  MagnifyingGlassIcon,
  MixerHorizontalIcon,
} from "@radix-ui/react-icons";

import { MenuHeading } from "./Menu.tsx";
import {
  DEFAULT_SEARCH_SOURCES,
  isChartSearchKind,
  isQuerylessSearchKind,
  SEARCH_LIMIT_OPTIONS,
  SEARCH_SOURCES,
  searchKindSupportsSource,
  usableSearchSources,
  type SearchBoxKind,
  type SearchSource,
  type TrackSource,
} from "./Types.tsx";
import { BEATPORT_GENRES } from "./BeatportGenres.tsx";

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
  beatportGenreId: number;
  setBeatportGenreId: (id: number) => void;
  searchBusy: boolean;
  errorMessage?: string;
}

const SEARCH_KIND_OPTIONS = [
  { value: "fuzzy", label: "Fuzzy" },
  { value: "by-label", label: "Label" },
  { value: "by-artist", label: "Artist" },
  { value: "new-releases", label: "Latest releases" },
  { value: "beatport-top-100", label: "Beatport Top 100" },
  { value: "traxsource-top-100", label: "Traxsource Top 100" },
];

const GENRE_OPTIONS = BEATPORT_GENRES.map((g) => ({
  value: `${g.id}`,
  label: g.name,
}));

const SOURCE_LABELS: Record<SearchSource, string> = {
  "apple-music": "Apple Music",
  beatport: "Beatport",
  soundcloud: "SoundCloud",
  spotify: "Spotify",
  traxsource: "Traxsource",
  volumo: "Volumo",
};

const ALL_SEARCH_SOURCES = SEARCH_SOURCES.map((value) => ({
  value,
  label: SOURCE_LABELS[value],
}));

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
    case "beatport-top-100":
      return "beatport-top-100";
    case "traxsource-top-100":
      return "traxsource-top-100";
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
  beatportGenreId,
  setBeatportGenreId,
  searchBusy,
  errorMessage,
}: ISearchBoxProps) {
  // A top-100 chart comes from a single store, so the sources option doesn't apply.
  const isTop100 = isChartSearchKind(searchKind);

  const toggleSearchSource = (source: TrackSource, checked: boolean) => {
    setSearchSources(
      checked
        ? [...searchSources, source]
        : searchSources.filter((s) => s !== source),
    );
  };

  const hasDefaultSources =
    searchSources.length === DEFAULT_SEARCH_SOURCES.length &&
    DEFAULT_SEARCH_SOURCES.every((s) => searchSources.includes(s));

  const sourceCount = usableSearchSources(searchKind, searchSources).length;
  const noSourceMessage =
    isTop100 || sourceCount
      ? undefined
      : searchSources.length
        ? `Spotify and Apple Music can't ${searchKind === "by-label" ? "search by label" : "find latest releases"}. Choose another source in Search options`
        : "Choose a source in Search options";

  const optionsSummary = isTop100
    ? `Up to ${searchLimit} results`
    : `Up to ${searchLimit} results from ${sourceCount} source${sourceCount === 1 ? "" : "s"}`;

  return (
    <HStack width="100%" gap={1} vAlign="center" className="search-box">
      <StackItem size="fill">
        <InputGroup
          label="Search"
          isLabelHidden
          status={
            errorMessage
              ? { type: "error", message: errorMessage }
              : noSourceMessage
                ? { type: "warning", message: noSourceMessage }
                : undefined
          }
        >
          <Selector
            label="Search kind"
            isLabelHidden
            options={SEARCH_KIND_OPTIONS}
            value={searchKind}
            onChange={(v) => setSearchKind(toSearchKind(v))}
            // Size to its label instead of splitting the group evenly.
            controlStyle={{ flex: "none", width: "auto" }}
          />
          {searchKind === "new-releases" ? (
            <Selector
              label="Released within"
              isLabelHidden
              options={DAY_OPTIONS.map((d) => ({
                value: `${d}`,
                label: `Last ${d} day${d === 1 ? "" : "s"}`,
              }))}
              value={`${newReleasesDays}`}
              onChange={(v) => setNewReleasesDays(parseInt(v))}
            />
          ) : searchKind === "beatport-top-100" ? (
            <Selector
              label="Genre"
              isLabelHidden
              options={GENRE_OPTIONS}
              value={`${beatportGenreId}`}
              onChange={(v) => setBeatportGenreId(parseInt(v))}
            />
          ) : searchKind === "traxsource-top-100" ? (
            // Traxsource's chart spans all genres, so there is nothing to pick here.
            <Selector
              label="Genre"
              isLabelHidden
              options={[{ value: "all", label: "All genres" }]}
              value="all"
              isDisabled
              disabledMessage="Traxsource's top 100 spans all genres"
            />
          ) : (
            <TextInput
              label="Search query"
              isLabelHidden
              placeholder="Search tracks, artists, labels…"
              hasClear
              value={value}
              onChange={setValue}
              onKeyDown={(ev) => {
                if (ev.key === "Enter") onSearch();
              }}
            />
          )}
        </InputGroup>
      </StackItem>

      <DropdownMenu
        hasChevron={false}
        menuWidth={200}
        button={{
          label: "Search options",
          icon: <MixerHorizontalIcon />,
          isIconOnly: true,
          variant: "ghost",
          tooltip: optionsSummary,
        }}
      >
        <MenuHeading>Result limit</MenuHeading>
        <DropdownMenuRadioGroup
          label="Result limit"
          value={`${searchLimit}`}
          onChange={(v) => setSearchLimit(parseInt(v))}
          hasCloseOnSelect={false}
        >
          {SEARCH_LIMIT_OPTIONS.map((n) => (
            <DropdownMenuRadioItem key={n} value={`${n}`} label={`${n}`} />
          ))}
        </DropdownMenuRadioGroup>

        {!isTop100 && (
          <>
            <Divider />

            <MenuHeading>Sources</MenuHeading>
            {ALL_SEARCH_SOURCES.map((s) => {
              const isSupported = searchKindSupportsSource(searchKind, s.value);
              return (
                <DropdownMenuCheckboxItem
                  key={s.value}
                  label={s.label}
                  description={
                    isSupported
                      ? undefined
                      : searchKind === "by-label"
                        ? "Can't search by label"
                        : "Not used for latest releases"
                  }
                  // The selection is kept for the other search kinds.
                  value={isSupported && searchSources.includes(s.value)}
                  isDisabled={!isSupported}
                  onChange={(checked) => toggleSearchSource(s.value, checked)}
                />
              );
            })}
            <Divider />
            <DropdownMenuItem
              label="Reset to defaults"
              isDisabled={hasDefaultSources}
              hasCloseOnSelect={false}
              onClick={() => setSearchSources([...DEFAULT_SEARCH_SOURCES])}
            />
          </>
        )}
      </DropdownMenu>

      <IconButton
        variant="primary"
        label="Search"
        tooltip="Search"
        icon={<MagnifyingGlassIcon />}
        isLoading={searchBusy}
        isDisabled={
          (!isQuerylessSearchKind(searchKind) && value.trim().length === 0) ||
          noSourceMessage !== undefined
        }
        onClick={onSearch}
      />
    </HStack>
  );
}

export default SearchBox;
