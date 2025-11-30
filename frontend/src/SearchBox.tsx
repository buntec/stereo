import {
  Select,
  IconButton,
  Spinner,
  TextField,
  Flex,
  Tooltip,
  Text,
} from "@radix-ui/themes";
import { Cross1Icon, MagnifyingGlassIcon } from "@radix-ui/react-icons";

import { type SearchKind } from "./Types.tsx";

export interface ISearchBoxProps {
  value: string;
  setValue: (value: string) => void;
  searchKind: SearchKind;
  setSearchKind: (value: SearchKind) => void;
  searchLimit: number;
  setSearchLimit: (value: number) => void;
  searchBusy: boolean;
  errorMessage?: string;
}

function toSearchKind(s: string): SearchKind {
  switch (s) {
    case "fuzzy":
      return "fuzzy";
    case "by-label":
      return "by-label";
    case "by-artist":
      return "by-artist";
  }

  throw new Error(`cannot convert ${s} to search kind!`);
}

export function SearchBox({
  value,
  setValue,
  searchKind,
  setSearchKind,
  searchLimit,
  setSearchLimit,
  searchBusy,
  errorMessage,
}: ISearchBoxProps) {
  return (
    <Flex className="search-box" direction="column" gap="2" align="start" m="2">
      <TextField.Root
        className="search-box-text-field"
        size="2"
        variant="soft"
        value={value}
        onChange={(ev) => setValue(ev.target.value)}
      >
        <TextField.Slot>
          <MagnifyingGlassIcon height="16" width="16" />
        </TextField.Slot>
        <TextField.Slot>
          {searchBusy ? <Spinner size="2" /> : null}
        </TextField.Slot>

        <Tooltip content="Search by label, by artist, or fuzzy">
          <TextField.Slot side="right">
            <Select.Root
              defaultValue="fuzzy"
              value={searchKind}
              onValueChange={(value: string) =>
                setSearchKind(toSearchKind(value))
              }
            >
              <Select.Trigger variant="ghost" />
              <Select.Content>
                <Select.Item value="fuzzy">Fuzzy</Select.Item>
                <Select.Item value="by-label">Label</Select.Item>
                <Select.Item value="by-artist">Artist</Select.Item>
              </Select.Content>
            </Select.Root>
          </TextField.Slot>
        </Tooltip>

        <Tooltip content="Limit number of search results">
          <TextField.Slot side="right">
            <Select.Root
              defaultValue="25"
              value={`${searchLimit}`}
              onValueChange={(value: string) => setSearchLimit(parseInt(value))}
            >
              <Select.Trigger variant="ghost" />
              <Select.Content>
                <Select.Item value="10">10</Select.Item>
                <Select.Item value="25">25</Select.Item>
                <Select.Item value="50">50</Select.Item>
                <Select.Item value="100">100</Select.Item>
              </Select.Content>
            </Select.Root>
          </TextField.Slot>
        </Tooltip>

        <TextField.Slot side="right">
          <IconButton variant="ghost">
            <Cross1Icon onClick={() => setValue("")} height="16" width="16" />
          </IconButton>
        </TextField.Slot>
      </TextField.Root>
      {errorMessage ? <Text color="red">{errorMessage}</Text> : null}
    </Flex>
  );
}
