import { useState, useCallback, useEffect, useRef } from "react";
import { Button } from "@astryxdesign/core/Button";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Tooltip } from "@astryxdesign/core/Tooltip";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack, VStack, StackItem } from "@astryxdesign/core/Stack";

import { Pencil1Icon, MagicWandIcon } from "@radix-ui/react-icons";
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
  const [dialogOpen, setDialogOpen] = useState(false);
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

  const closeDialog = () => setDialogOpen(false);

  return (
    <>
      <IconButton
        variant="ghost"
        size="sm"
        label="Edit track"
        tooltip="Edit track"
        icon={<Pencil1Icon />}
        onClick={() => {
          resetValue();
          setDialogOpen(true);
        }}
      />

      <Dialog isOpen={dialogOpen} onOpenChange={setDialogOpen} purpose="form">
        <Layout
          header={
            <DialogHeader
              title="Edit"
              subtitle="Edit track meta data"
              onOpenChange={closeDialog}
              endContent={
                <Tooltip content="Fill in missing fields from Beatport, Traxsource, and Volumo">
                  <Button
                    variant="secondary"
                    size="sm"
                    icon={<MagicWandIcon />}
                    label="Fixup"
                    isLoading={isFixingUp}
                    onClick={fixup}
                  />
                </Tooltip>
              }
            />
          }
          content={
            <LayoutContent>
              <VStack
                gap={2}
                style={
                  isValid === false
                    ? { backgroundColor: "var(--color-error-muted)" }
                    : undefined
                }
              >
                <HStack gap={2} vAlign="end">
                  <img
                    src={`https://i.ytimg.com/vi/${value.yt_id}/default.jpg`}
                    alt="YouTube Video Thumbnail"
                    width="40"
                  />
                  <StackItem size="fill">
                    <TextInput
                      label="YouTube video ID (primary key)"
                      value={value.yt_id}
                      onChange={(v) => setValue({ ...value, yt_id: v })}
                    />
                  </StackItem>
                </HStack>

                <TextInput
                  label="Title"
                  value={value.title}
                  onChange={(v) => setValue({ ...value, title: v })}
                />

                <TextInput
                  label="Artists"
                  value={value.artists.join(",")}
                  onChange={(v) =>
                    setValue({
                      ...value,
                      artists: v.split(",").map((s) => s.trim()),
                    })
                  }
                />

                <TextInput
                  label="Release date"
                  value={value.release_date ?? ""}
                  onChange={(v) =>
                    setValue({ ...value, release_date: v || undefined })
                  }
                />

                <TextInput
                  label="Label"
                  value={value.label ?? ""}
                  onChange={(v) => setValue({ ...value, label: v })}
                />

                <TextInput
                  label="Genre"
                  value={value.genre ?? ""}
                  onChange={(v) => setValue({ ...value, genre: v })}
                />

                <TextInput
                  label="BPM"
                  value={value.bpm !== undefined ? `${value.bpm}` : ""}
                  onChange={(v) =>
                    setValue({
                      ...value,
                      bpm: v ? parseInt(v) : undefined,
                    })
                  }
                />

                <TextInput
                  label="Key"
                  value={value.key ?? ""}
                  onChange={(v) => setValue({ ...value, key: v })}
                />

                <TextInput
                  label="Beatport ID"
                  value={value.bp_id !== undefined ? `${value.bp_id}` : ""}
                  onChange={(v) =>
                    setValue({
                      ...value,
                      bp_id: v ? parseInt(v) : undefined,
                    })
                  }
                />

                <TextInput
                  label="Spotify ID"
                  value={value.sp_id ?? ""}
                  onChange={(v) =>
                    setValue({ ...value, sp_id: v || undefined })
                  }
                  isLoading={spotifyIdStatus === "checking"}
                  status={
                    spotifyIdStatus === "valid"
                      ? { type: "success", message: "Track exists on Spotify" }
                      : spotifyIdStatus === "invalid"
                        ? {
                            type: "error",
                            message: "No track with this ID on Spotify",
                          }
                        : undefined
                  }
                />

                <TextInput
                  label="Music Brainz ID"
                  value={value.mb_id ?? ""}
                  onChange={(v) => setValue({ ...value, mb_id: v })}
                />
              </VStack>
            </LayoutContent>
          }
          footer={
            <LayoutFooter>
              <HStack gap={2} hAlign="end">
                <Button
                  label="Cancel"
                  variant="secondary"
                  onClick={() => {
                    resetValue();
                    closeDialog();
                  }}
                />
                <Button
                  label="Save"
                  variant="primary"
                  isDisabled={!isValid}
                  onClick={() => {
                    commit();
                    closeDialog();
                  }}
                />
              </HStack>
            </LayoutFooter>
          }
        />
      </Dialog>
    </>
  );
}
