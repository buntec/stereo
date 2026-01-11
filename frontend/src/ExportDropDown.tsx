import { Tooltip, DropdownMenu, IconButton, Flex } from "@radix-ui/themes";
import { Share2Icon } from "@radix-ui/react-icons";

type ExportDropDownProps = {
  recentCollections: string[];
  exportToCollection: (coll: string) => void;
  exportToYTM: () => void;
  disabled: boolean;
};

function ExportDropDown({
  recentCollections,
  exportToCollection,
  exportToYTM,
  disabled,
}: ExportDropDownProps) {
  return (
    <DropdownMenu.Root>
      <Tooltip content="Export selected tracks">
        <DropdownMenu.Trigger disabled={disabled}>
          <IconButton variant="soft">
            <Share2Icon />
          </IconButton>
        </DropdownMenu.Trigger>
      </Tooltip>
      <DropdownMenu.Content>
        <DropdownMenu.Sub>
          <DropdownMenu.SubTrigger>
            Export selection to collection
          </DropdownMenu.SubTrigger>
          <DropdownMenu.SubContent>
            {recentCollections.map((coll: string, i: number) => (
              <DropdownMenu.Item
                key={i}
                onSelect={() => exportToCollection(coll)}
              >
                <Flex justify="between" align="center" gapX="4" width="100%">
                  {coll}
                </Flex>
              </DropdownMenu.Item>
            ))}
          </DropdownMenu.SubContent>
        </DropdownMenu.Sub>
        <DropdownMenu.Separator />
        <DropdownMenu.Item onSelect={exportToYTM}>
          Export selection to YouTube Music playlist (50 tracks max)
        </DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Root>
  );
}

export default ExportDropDown;
