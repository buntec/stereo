import {
  Select,
  IconButton,
  Spinner,
  TextField,
  Flex,
  Tooltip,
  Text,
  DropdownMenu,
} from "@radix-ui/themes";
import {
  Cross1Icon,
  MagnifyingGlassIcon,
  MixerHorizontalIcon,
} from "@radix-ui/react-icons";

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

const ALL_SEARCH_SOURCES: { value: TrackSource; label: string }[] = [
  { value: "beatport", label: "Beatport" },
  { value: "soundcloud", label: "SoundCloud" },
  { value: "volumo", label: "Volumo" },
  { value: "traxsource", label: "Traxsource" },
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
  const toggleSource = (source: TrackSource) => {
    if (searchSources.includes(source)) {
      setSearchSources(searchSources.filter((s) => s !== source));
    } else {
      setSearchSources([...searchSources, source]);
    }
  };

  const isNewReleases = searchKind === "new-releases";

  const kindSelect = (
    <Select.Root
      defaultValue="fuzzy"
      value={searchKind}
      onValueChange={(value: string) => setSearchKind(toSearchKind(value))}
    >
      <Select.Trigger variant="soft" />
      <Select.Content>
        <Select.Item value="fuzzy">Fuzzy</Select.Item>
        <Select.Item value="by-label">Label</Select.Item>
        <Select.Item value="by-artist">Artist</Select.Item>
        <Select.Item value="new-releases">Latest releases</Select.Item>
      </Select.Content>
    </Select.Root>
  );

  const limitSelect = (
    <Select.Root
      defaultValue="100"
      value={`${searchLimit}`}
      onValueChange={(value: string) => setSearchLimit(parseInt(value))}
    >
      <Select.Trigger variant="ghost" />
      <Select.Content>
        <Select.Item value="10">10</Select.Item>
        <Select.Item value="25">25</Select.Item>
        <Select.Item value="50">50</Select.Item>
        <Select.Item value="100">100</Select.Item>
        <Select.Item value="500">500</Select.Item>
        <Select.Item value="1000">1000</Select.Item>
      </Select.Content>
    </Select.Root>
  );

  const sourcesMenu = (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger>
        <IconButton variant="ghost">
          <MixerHorizontalIcon height="16" width="16" />
        </IconButton>
      </DropdownMenu.Trigger>
      <DropdownMenu.Content>
        {ALL_SEARCH_SOURCES.map(({ value: source, label }) => (
          <DropdownMenu.CheckboxItem
            key={source}
            checked={searchSources.includes(source)}
            onSelect={(ev) => ev.preventDefault()}
            onCheckedChange={() => toggleSource(source)}
          >
            {label}
          </DropdownMenu.CheckboxItem>
        ))}
      </DropdownMenu.Content>
    </DropdownMenu.Root>
  );

  return (
    <Flex className="search-box" align="center" gap="2" m="2">
      <Tooltip content="Search by label, by artist, fuzzy, or latest releases">
        {kindSelect}
      </Tooltip>
      {isNewReleases ? (
        <Flex
          className="search-box-text-field"
          align="center"
          gap="2"
          px="2"
        >
          <MagnifyingGlassIcon height="16" width="16" />
          {searchBusy ? <Spinner size="2" /> : null}
          <Select.Root
            value={`${newReleasesDays}`}
            onValueChange={(value: string) =>
              setNewReleasesDays(parseInt(value))
            }
          >
            <Select.Trigger variant="soft" style={{ flexGrow: 1 }} />
            <Select.Content>
              {DAY_OPTIONS.map((d) => (
                <Select.Item key={d} value={`${d}`}>
                  Last {d} day{d === 1 ? "" : "s"}
                </Select.Item>
              ))}
            </Select.Content>
          </Select.Root>
          <Tooltip content="Limit number of search results">
            {limitSelect}
          </Tooltip>
          <Tooltip content="Search sources">{sourcesMenu}</Tooltip>
        </Flex>
      ) : (
        <TextField.Root
          className="search-box-text-field"
          size="2"
          variant="soft"
          value={value}
          onChange={(ev) => setValue(ev.target.value)}
          onKeyDown={(ev) => {
            if (ev.key === "Enter") onSearch();
          }}
        >
          <TextField.Slot>
            <MagnifyingGlassIcon height="16" width="16" />
          </TextField.Slot>
          <TextField.Slot>
            {searchBusy ? <Spinner size="2" /> : null}
          </TextField.Slot>

          <Tooltip content="Limit number of search results">
            <TextField.Slot side="right">{limitSelect}</TextField.Slot>
          </Tooltip>
          <Tooltip content="Search sources">
            <TextField.Slot side="right">{sourcesMenu}</TextField.Slot>
          </Tooltip>

          {value.length > 0 && (
            <Tooltip content="Cancel search">
              <TextField.Slot side="right">
                <IconButton variant="ghost">
                  <Cross1Icon
                    onClick={() => setValue("")}
                    height="16"
                    width="16"
                  />
                </IconButton>
              </TextField.Slot>
            </Tooltip>
          )}
        </TextField.Root>
      )}
      <Tooltip content="Search">
        <IconButton
          variant="soft"
          disabled={!isNewReleases && value.trim().length === 0}
          onClick={onSearch}
        >
          <MagnifyingGlassIcon height="16" width="16" />
        </IconButton>
      </Tooltip>
      {errorMessage ? <Text color="red">{errorMessage}</Text> : null}
    </Flex>
  );
}

export default SearchBox;
