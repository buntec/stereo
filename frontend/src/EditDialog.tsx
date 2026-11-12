import { useState, useCallback, useEffect, useRef } from "react";
import { Button } from "@astryxdesign/core/Button";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Tooltip } from "@astryxdesign/core/Tooltip";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { FormLayout } from "@astryxdesign/core/FormLayout";
import type { ISODateString } from "@astryxdesign/core/Calendar";
import { DateInput } from "@astryxdesign/core/DateInput";
import { NumberInput } from "@astryxdesign/core/NumberInput";
import { Collapsible } from "@astryxdesign/core/Collapsible";
import { Banner } from "@astryxdesign/core/Banner";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack, StackItem } from "@astryxdesign/core/Stack";

import { Pencil1Icon, MagicWandIcon } from "@radix-ui/react-icons";
import type { ITrack, RequestReply } from "./Types";

const SPOTIFY_ID_CHECK_DEBOUNCE_MS = 500;
const isCalendarDate = (date: string | undefined): date is ISODateString =>
  Boolean(date && /^\d{4}-\d{2}-\d{2}$/.test(date));

type SpotifyIdStatus = "idle" | "checking" | "valid" | "invalid";

type EditTrackDialogProps = {
  value: ITrack;
  isValid: boolean | null;
  setValue: (track: ITrack) => void;
  resetValue: () => void;
  commit: () => void;
  requestReply: RequestReply;
  // Open state, when the parent opens the dialog itself (e.g. from a menu). Without it
  // the dialog renders its own edit button. The parent resets the value when opening.
  isOpen?: boolean;
  onOpenChange?: (isOpen: boolean) => void;
};

export function EditTrackDialog({
  value,
  isValid,
  setValue,
  resetValue,
  commit,
  requestReply,
  isOpen,
  onOpenChange,
}: EditTrackDialogProps) {
  const isControlled = isOpen !== undefined;
  const [uncontrolledOpen, setUncontrolledOpen] = useState(false);
  const dialogOpen = isControlled ? isOpen : uncontrolledOpen;
  const setDialogOpen = (open: boolean) => {
    if (!isControlled) setUncontrolledOpen(open);
    onOpenChange?.(open);
  };
  const [isFixingUp, setIsFixingUp] = useState(false);

  // Partial release dates (YYYY, YYYY-MM) need a free-form field. Once the dialog has
  // shown one it keeps it until reopened, so typing or clearing never swaps inputs
  // (and focus) mid-edit.
  const [freeformDate, setFreeformDate] = useState(false);
  const hasPartialDate =
    !!value.release_date && !isCalendarDate(value.release_date);
  if (hasPartialDate && !freeformDate) setFreeformDate(true);

  // Reset on every opening, also when the parent opens the dialog.
  const [wasOpen, setWasOpen] = useState(dialogOpen);
  if (dialogOpen !== wasOpen) {
    setWasOpen(dialogOpen);
    if (dialogOpen) setFreeformDate(hasPartialDate);
  }

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
      {!isControlled && (
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
      )}

      <Dialog isOpen={dialogOpen} onOpenChange={setDialogOpen} purpose="form">
        <Layout
          header={
            <DialogHeader
              title="Edit track"
              subtitle="Update the details saved in your collection."
              onOpenChange={closeDialog}
              endContent={
                <Tooltip content="Fill in missing fields from Beatport, Traxsource, and Volumo">
                  <Button
                    variant="secondary"
                    size="sm"
                    icon={<MagicWandIcon />}
                    label="Fill missing metadata"
                    isLoading={isFixingUp}
                    onClick={fixup}
                  />
                </Tooltip>
              }
            />
          }
          content={
            <LayoutContent>
              <FormLayout>
                {isValid === false && (
                  <Banner
                    status="error"
                    title="Check track details"
                    description="Enter a title and YouTube video ID, and check the formats of the other fields."
                  />
                )}
                <HStack gap={2} vAlign="end">
                  <img
                    src={`https://i.ytimg.com/vi/${value.yt_id}/default.jpg`}
                    alt="YouTube Video Thumbnail"
                    width="40"
                  />
                  <StackItem size="fill">
                    <TextInput
                      label="YouTube video ID"
                      isRequired
                      status={
                        !value.yt_id.trim()
                          ? {
                              type: "error",
                              message:
                                "Enter the ID of the YouTube video used for playback.",
                            }
                          : undefined
                      }
                      value={value.yt_id}
                      onChange={(v) => setValue({ ...value, yt_id: v })}
                    />
                  </StackItem>
                </HStack>

                <TextInput
                  label="Title"
                  isRequired
                  status={
                    !value.title.trim()
                      ? { type: "error", message: "Enter a track title." }
                      : undefined
                  }
                  value={value.title}
                  onChange={(v) => setValue({ ...value, title: v })}
                />

                <TextInput
                  label="Artists"
                  description="Separate multiple artists with commas."
                  value={value.artists.join(",")}
                  onChange={(v) =>
                    setValue({
                      ...value,
                      artists: v.split(",").map((s) => s.trim()),
                    })
                  }
                />

                {freeformDate ? (
                  <TextInput
                    label="Release date"
                    description="Use YYYY, YYYY-MM, or YYYY-MM-DD."
                    value={value.release_date ?? ""}
                    onChange={(release_date) =>
                      setValue({
                        ...value,
                        release_date: release_date || undefined,
                      })
                    }
                  />
                ) : (
                  <DateInput
                    label="Release date"
                    hasClear
                    value={
                      isCalendarDate(value.release_date)
                        ? value.release_date
                        : undefined
                    }
                    onChange={(v) =>
                      setValue({ ...value, release_date: v || undefined })
                    }
                  />
                )}

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

                <NumberInput
                  label="Tempo"
                  value={value.bpm}
                  min={1}
                  max={999}
                  isIntegerOnly
                  hasClear
                  units="BPM"
                  onChange={(bpm) =>
                    setValue({ ...value, bpm: bpm ?? undefined })
                  }
                />

                <TextInput
                  label="Key"
                  value={value.key ?? ""}
                  onChange={(v) => setValue({ ...value, key: v })}
                />

                <Collapsible
                  trigger="Advanced identifiers"
                  defaultIsOpen={false}
                >
                  <FormLayout>
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
                          ? {
                              type: "success",
                              message: "Track exists on Spotify",
                            }
                          : spotifyIdStatus === "invalid"
                            ? {
                                type: "error",
                                message: "No track with this ID on Spotify",
                              }
                            : undefined
                      }
                    />

                    <TextInput
                      label="MusicBrainz ID"
                      value={value.mb_id ?? ""}
                      onChange={(v) => setValue({ ...value, mb_id: v })}
                    />
                  </FormLayout>
                </Collapsible>
              </FormLayout>
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
                  isDisabled={
                    !isValid || !value.title.trim() || !value.yt_id.trim()
                  }
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
