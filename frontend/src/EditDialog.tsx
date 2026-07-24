import { useState, useCallback, useEffect, useRef } from "react";
import {
  Text,
  Tooltip,
  Button,
  IconButton,
  Dialog,
  TextField,
  Flex,
  Spinner,
} from "@radix-ui/themes";

import {
  Pencil1Icon,
  MagicWandIcon,
  CheckCircledIcon,
  CrossCircledIcon,
} from "@radix-ui/react-icons";
import type { ITrack, RequestReply } from "./Types";

const SPOTIFY_ID_CHECK_DEBOUNCE_MS = 500;

type SpotifyIdStatus = "idle" | "checking" | "valid" | "invalid";

type EditTrackDialogProps = {
  value: ITrack;
  isValid: boolean | null;
  setValue: (track: ITrack) => void;
  resetValue: () => void;
  commit: () => void;
  requestReply: RequestReply;
};

export function EditTrackDialog({
  value,
  isValid,
  setValue,
  resetValue,
  commit,
  requestReply,
}: EditTrackDialogProps) {
  const [isFixingUp, setIsFixingUp] = useState(false);

  const [spotifyIdStatus, setSpotifyIdStatus] =
    useState<SpotifyIdStatus>("idle");
  const spotifyIdTimer = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  );

  useEffect(() => {
    clearTimeout(spotifyIdTimer.current);
    const sp_id = value.sp_id;
    if (!sp_id) {
      setSpotifyIdStatus("idle");
      return;
    }
    setSpotifyIdStatus("checking");
    spotifyIdTimer.current = setTimeout(() => {
      requestReply({ type: "check-spotify-id", sp_id }, (msg) => {
        if ("exists" in msg && msg.sp_id === sp_id) {
          setSpotifyIdStatus(msg.exists ? "valid" : "invalid");
        }
      });
    }, SPOTIFY_ID_CHECK_DEBOUNCE_MS);
    return () => clearTimeout(spotifyIdTimer.current);
  }, [value.sp_id, requestReply]);

  const fixup = useCallback(() => {
    setIsFixingUp(true);
    requestReply({ type: "fixup-track", track: value }, (msg) => {
      setIsFixingUp(false);
      if ("track" in msg && msg.track) {
        setValue(msg.track);
      }
    });
  }, [requestReply, value, setValue]);

  return (
    <Dialog.Root>
      <Tooltip content="Edit track">
        <Dialog.Trigger>
          <IconButton variant="ghost" size="1" onClick={resetValue}>
            <Pencil1Icon />
          </IconButton>
        </Dialog.Trigger>
      </Tooltip>

      <Dialog.Content onOpenAutoFocus={(ev) => ev.preventDefault()}>
        <Flex justify="between" align="center">
          <Dialog.Title mb="0">Edit</Dialog.Title>
          <Tooltip content="Fill in missing fields from Beatport, Traxsource, and Volumo">
            <Button
              variant="soft"
              size="1"
              onClick={fixup}
              disabled={isFixingUp}
            >
              <MagicWandIcon />
              Fixup
            </Button>
          </Tooltip>
        </Flex>

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
                setValue({
                  ...value,
                  release_date: ev.target.value || undefined,
                })
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
                setValue({
                  ...value,
                  bpm: ev.target.value ? parseInt(ev.target.value) : undefined,
                })
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
                setValue({
                  ...value,
                  bp_id: ev.target.value
                    ? parseInt(ev.target.value)
                    : undefined,
                })
              }
            />
          </label>

          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Spotify ID
            </Text>
            <TextField.Root
              value={value.sp_id ?? ""}
              onChange={(ev) =>
                setValue({ ...value, sp_id: ev.target.value || undefined })
              }
            >
              {spotifyIdStatus !== "idle" && (
                <TextField.Slot>
                  {spotifyIdStatus === "checking" && <Spinner size="1" />}
                  {spotifyIdStatus === "valid" && (
                    <Tooltip content="Track exists on Spotify">
                      <CheckCircledIcon color="green" />
                    </Tooltip>
                  )}
                  {spotifyIdStatus === "invalid" && (
                    <Tooltip content="No track with this ID on Spotify">
                      <CrossCircledIcon color="red" />
                    </Tooltip>
                  )}
                </TextField.Slot>
              )}
            </TextField.Root>
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
