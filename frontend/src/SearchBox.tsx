import { Selector } from "@astryxdesign/core/Selector";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
} from "@astryxdesign/core/DropdownMenu";
import { InputGroup } from "@astryxdesign/core/InputGroup";
import { TextInput } from "@astryxdesign/core/TextInput";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Divider } from "@astryxdesign/core/Divider";
import { HStack } from "@astryxdesign/core/Stack";
import {
  MagnifyingGlassIcon,
  MixerHorizontalIcon,
} from "@radix-ui/react-icons";

import { MenuCheck, MenuHeading } from "./Menu.tsx";
import {
  isQuerylessSearchKind,
  type SearchBoxKind,
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
];

const GENRE_OPTIONS = BEATPORT_GENRES.map((g) => ({
  value: `${g.id}`,
  label: g.name,
}));

const LIMIT_OPTIONS = [10, 25, 50, 100, 500, 1000];

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
    case "beatport-top-100":
      return "beatport-top-100";
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
  // The top-100 chart comes from Beatport alone, so the sources option doesn't apply.
  const isTop100 = searchKind === "beatport-top-100";

  const toggleSearchSource = (source: TrackSource, checked: boolean) => {
    setSearchSources(
      checked
        ? [...searchSources, source]
        : searchSources.filter((s) => s !== source),
    );
  };

  const optionsSummary = isTop100
    ? `Up to ${searchLimit} results`
    : `Up to ${searchLimit} results from ${searchSources.length} source${searchSources.length === 1 ? "" : "s"}`;

  return (
    <HStack gap={1} vAlign="center" className="search-box">
      <InputGroup
        label="Search"
        isLabelHidden
        className="search-box-group"
        status={
          errorMessage ? { type: "error", message: errorMessage } : undefined
        }
      >
        <Selector
          label="Search kind"
          isLabelHidden
          options={SEARCH_KIND_OPTIONS}
          value={searchKind}
          onChange={(v) => setSearchKind(toSearchKind(v))}
          // Size to its label instead of splitting the group evenly.
          style={{ flex: "none", width: "auto" }}
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
        ) : isTop100 ? (
          <Selector
            label="Genre"
            isLabelHidden
            options={GENRE_OPTIONS}
            value={`${beatportGenreId}`}
            onChange={(v) => setBeatportGenreId(parseInt(v))}
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
          aria-label="Result limit"
          value={`${searchLimit}`}
          onChange={(v) => setSearchLimit(parseInt(v))}
          hasCloseOnSelect={false}
        >
          {LIMIT_OPTIONS.map((n) => (
            <DropdownMenuRadioItem
              key={n}
              value={`${n}`}
              label={`${n}`}
              endContent={<MenuCheck isChecked={n === searchLimit} />}
            />
          ))}
        </DropdownMenuRadioGroup>

        {!isTop100 && (
          <>
            <Divider />

            <MenuHeading>Sources</MenuHeading>
            {ALL_SEARCH_SOURCES.map((s) => (
              <DropdownMenuCheckboxItem
                key={s.value}
                label={s.label}
                value={searchSources.includes(s.value)}
                onChange={(checked) => toggleSearchSource(s.value, checked)}
                endContent={
                  <MenuCheck isChecked={searchSources.includes(s.value)} />
                }
              />
            ))}
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
          !isQuerylessSearchKind(searchKind) && value.trim().length === 0
        }
        onClick={onSearch}
      />
    </HStack>
  );
}

export default SearchBox;
