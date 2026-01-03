import {
  Text,
  Tooltip,
  Button,
  IconButton,
  Dialog,
  TextField,
  Flex,
} from "@radix-ui/themes";

import { Pencil1Icon } from "@radix-ui/react-icons";
import type { ITrack } from "./Types";

type EditTrackDialogProps = {
  value: ITrack;
  isValid: boolean | null;
  setValue: (track: ITrack) => void;
  resetValue: () => void;
  commit: () => void;
};

export function EditTrackDialog({
  value,
  isValid,
  setValue,
  resetValue,
  commit,
}: EditTrackDialogProps) {
  return (
    <Dialog.Root>
      <Tooltip content="Edit track">
        <Dialog.Trigger>
          <IconButton variant="ghost" size="1" onClick={resetValue}>
            <Pencil1Icon />
          </IconButton>
        </Dialog.Trigger>
      </Tooltip>

      <Dialog.Content>
        <Dialog.Title>Edit</Dialog.Title>

        <Dialog.Description>Edit track meta data</Dialog.Description>

        <Flex
          direction="column"
          my="2"
          gap="2"
          className={
            isValid
              ? "edit-track valid"
              : isValid === null
                ? "edit-track"
                : "edit-track invalid"
          }
        >
          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              YouTube video ID (primary key)
            </Text>
            <TextField.Root
              value={value.yt_id}
              onChange={(ev) => setValue({ ...value, yt_id: ev.target.value })}
            >
              <TextField.Slot>
                <img
                  src={`https://i.ytimg.com/vi/${value.yt_id}/default.jpg`}
                  alt="YouTube Video Thumbnail"
                  width="40"
                />
              </TextField.Slot>
            </TextField.Root>
          </label>

          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Title
            </Text>
            <TextField.Root
              value={value.title}
              onChange={(ev) => setValue({ ...value, title: ev.target.value })}
            />
          </label>

          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Artists
            </Text>
            <TextField.Root
              value={value.artists.join(",")}
              onChange={(ev) =>
                setValue({
                  ...value,
                  artists: ev.target.value.split(",").map((s) => s.trim()),
                })
              }
            />
          </label>

          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Release date
            </Text>
            <TextField.Root
              value={value.release_date ?? ""}
              onChange={(ev) =>
                setValue({ ...value, release_date: ev.target.value })
              }
            />
          </label>

          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Label
            </Text>
            <TextField.Root
              value={value.label ?? ""}
              onChange={(ev) => setValue({ ...value, label: ev.target.value })}
            />
          </label>

          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Genre
            </Text>
            <TextField.Root
              value={value.genre ?? ""}
              onChange={(ev) => setValue({ ...value, genre: ev.target.value })}
            />
          </label>

          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              BPM
            </Text>
            <TextField.Root
              value={value.bpm ?? ""}
              onChange={(ev) =>
                setValue({ ...value, bpm: parseInt(ev.target.value) })
              }
            />
          </label>

          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Key
            </Text>
            <TextField.Root
              value={value.key ?? ""}
              onChange={(ev) => setValue({ ...value, key: ev.target.value })}
            />
          </label>

          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Beatport ID
            </Text>
            <TextField.Root
              value={value.bp_id ?? ""}
              onChange={(ev) =>
                setValue({ ...value, bp_id: parseInt(ev.target.value) })
              }
            />
          </label>

          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Music Brainz ID
            </Text>
            <TextField.Root
              value={value.mb_id ?? ""}
              onChange={(ev) => setValue({ ...value, mb_id: ev.target.value })}
            />
          </label>
        </Flex>

        <Flex gap="3" mt="4" justify="end">
          <Dialog.Close>
            <Button variant="soft" color="gray" onClick={resetValue}>
              Cancel
            </Button>
          </Dialog.Close>
          <Dialog.Close>
            <Button onClick={commit} disabled={!isValid} color="green">
              Save
            </Button>
          </Dialog.Close>
        </Flex>
      </Dialog.Content>
    </Dialog.Root>
  );
}
