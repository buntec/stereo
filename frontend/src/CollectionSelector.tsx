import { Tooltip, IconButton, TextField, Flex, Code } from "@radix-ui/themes";
import { ResetIcon, PlusCircledIcon, CubeIcon } from "@radix-ui/react-icons";

export interface CollectionSelectorProps {
  setValue: (value: string) => void;
  resetToDefault: () => void;
  value: string;
  isValid: boolean;
  suggestions: string[];
  size?: number;
  createCollection: (path: string) => void;
}

export function CollectionSelector({
  isValid,
  value,
  setValue,
  resetToDefault,
  suggestions,
  size,
  createCollection,
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
          <CubeIcon color={isValid ? "green" : "red"} height="16" width="16" />
        </TextField.Slot>
        <TextField.Slot>
          <Tooltip content="Reset to default collection">
            <IconButton variant="ghost" onClick={resetToDefault}>
              <ResetIcon />
            </IconButton>
          </Tooltip>
        </TextField.Slot>
        <TextField.Slot>
          {isValid ? (
            <Code>{size}</Code>
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
