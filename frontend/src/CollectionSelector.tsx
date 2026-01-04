import {
  DropdownMenu,
  Tooltip,
  IconButton,
  TextField,
  Flex,
  Code,
} from "@radix-ui/themes";
import { ResetIcon, PlusCircledIcon, CubeIcon } from "@radix-ui/react-icons";

type DropDownProps = {
  isValid: boolean;
  recentCollections: string[];
  selectCollection: (collection: string) => void;
  clearRecent: () => void;
};

function DropDown({
  isValid,
  recentCollections,
  clearRecent,
  selectCollection,
}: DropDownProps) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger>
        <IconButton variant="ghost">
          <CubeIcon color={isValid ? "green" : "red"} height="16" width="16" />
        </IconButton>
      </DropdownMenu.Trigger>
      <DropdownMenu.Content>
        <DropdownMenu.Sub>
          <DropdownMenu.SubTrigger>Recent collections</DropdownMenu.SubTrigger>
          <DropdownMenu.SubContent>
            {recentCollections.map((coll: string, i: number) => (
              <DropdownMenu.Item
                key={i}
                onSelect={() => selectCollection(coll)}
              >
                {coll}
              </DropdownMenu.Item>
            ))}
            <DropdownMenu.Separator />
            <DropdownMenu.Item onSelect={clearRecent}>Clear</DropdownMenu.Item>
          </DropdownMenu.SubContent>
        </DropdownMenu.Sub>
      </DropdownMenu.Content>
    </DropdownMenu.Root>
  );
}

export type CollectionSelectorProps = {
  setValue: (value: string) => void;
  resetToDefault: () => void;
  value: string;
  isValid: boolean;
  suggestions: string[];
  size?: number;
  createCollection: (path: string) => void;
  recentCollections: string[];
  clearRecentCollections: () => void;
};

function CollectionSelector({
  isValid,
  value,
  setValue,
  resetToDefault,
  suggestions,
  size,
  createCollection,
  recentCollections,
  clearRecentCollections,
}: CollectionSelectorProps) {
  return (
    <Flex className="collection-selector-box" align="center" gap="2" m="2">
      <TextField.Root
        list="collection-datalist"
        className="collection-text-field"
        size="2"
        variant="soft"
        value={value}
        onChange={(ev) => setValue(ev.target.value)}
      >
        <TextField.Slot>
          <DropDown
            isValid={isValid}
            recentCollections={recentCollections}
            clearRecent={clearRecentCollections}
            selectCollection={setValue}
          />
        </TextField.Slot>
        <TextField.Slot>
          <Tooltip content="Return to default collection">
            <IconButton variant="ghost" onClick={resetToDefault}>
              <ResetIcon />
            </IconButton>
          </Tooltip>
        </TextField.Slot>
        <TextField.Slot>
          {isValid ? (
            <Tooltip content={`Collection contains ${size} tracks`}>
              <Code>{size}</Code>
            </Tooltip>
          ) : (
            <Tooltip content="Create new collection at current path">
              <IconButton
                color="green"
                variant="ghost"
                onClick={() => createCollection(value)}
              >
                <PlusCircledIcon />
              </IconButton>
            </Tooltip>
          )}
        </TextField.Slot>
      </TextField.Root>

      <datalist id="collection-datalist">
        {suggestions.map((s: string) => (
          <option key={s} value={s}></option>
        ))}
      </datalist>
    </Flex>
  );
}

export default CollectionSelector;
