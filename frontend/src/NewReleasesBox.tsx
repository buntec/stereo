import { Select, IconButton, Spinner, Flex, Tooltip } from "@radix-ui/themes";
import { RocketIcon } from "@radix-ui/react-icons";

export interface INewReleasesBoxProps {
  days: number;
  setDays: (days: number) => void;
  onSearch: () => void;
  busy: boolean;
}

const DAY_OPTIONS = [1, 7, 14, 30, 60, 90, 180, 365];

function NewReleasesBox({ days, setDays, onSearch, busy }: INewReleasesBoxProps) {
  return (
    <Flex className="search-box new-releases-box" align="center" gap="2" m="2">
      <Select.Root
        size="2"
        value={`${days}`}
        onValueChange={(value: string) => setDays(parseInt(value))}
      >
        <Select.Trigger variant="soft" />
        <Select.Content>
          {DAY_OPTIONS.map((d) => (
            <Select.Item key={d} value={`${d}`}>
              Last {d} day{d === 1 ? "" : "s"}
            </Select.Item>
          ))}
        </Select.Content>
      </Select.Root>
      <Tooltip content="Search for new releases from top artists and labels">
        <IconButton variant="soft" disabled={busy} onClick={onSearch}>
          {busy ? <Spinner size="2" /> : <RocketIcon height="16" width="16" />}
        </IconButton>
      </Tooltip>
    </Flex>
  );
}

export default NewReleasesBox;
