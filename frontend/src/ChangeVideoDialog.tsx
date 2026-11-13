import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "@astryxdesign/core/Button";
import { Banner } from "@astryxdesign/core/Banner";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { RadioList, RadioListItem } from "@astryxdesign/core/RadioList";
import { Spinner } from "@astryxdesign/core/Spinner";
import { HStack, StackItem, VStack } from "@astryxdesign/core/Stack";
import { TextInput } from "@astryxdesign/core/TextInput";

import type { ITrack, RequestReply, YouTubeVideo } from "./Types.tsx";
import { formatTime } from "./Utils.tsx";
import YouTubeIcon from "./YouTubeIcon.tsx";

// The query the dialog starts with: what the automatic match searched for, roughly.
const defaultQuery = (track: ITrack) =>
  [track.artists.join(", "), track.title, track.mix_name]
    .filter(Boolean)
    .join(" ");

const describe = (video: YouTubeVideo) =>
  [
    video.channel,
    video.duration_secs != null ? formatTime(video.duration_secs) : "",
  ]
    .filter(Boolean)
    .join(" · ") || undefined;

type ChangeVideoDialogProps = {
  // The track whose video is changed; the dialog is open while it is set.
  track: ITrack | null;
  onClose: () => void;
  onChange: (track: ITrack, yt_id: string) => void;
  requestReply: RequestReply;
};

// Picks another YouTube video for a track whose automatic match is wrong: from a YouTube
// search, prefilled with the track's artists and title, or a pasted video link.
export default function ChangeVideoDialog({
  track,
  onClose,
  onChange,
  requestReply,
}: ChangeVideoDialogProps) {
  const [query, setQuery] = useState("");
  const [videos, setVideos] = useState<YouTubeVideo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isSearching, setIsSearching] = useState(false);
  const [picked, setPicked] = useState("");
  // Only the latest search's reply is shown.
  const searchId = useRef(0);

  // Sends a search; its reply replaces the videos shown, unless another search followed.
  const request = useCallback(
    (q: string) => {
      const id = ++searchId.current;
      const isLink = /^https?:\/\//i.test(q);
      requestReply(
        { type: "search-youtube-videos", query: q },
        (msg) => {
          if (id !== searchId.current) return;
          setIsSearching(false);
          if (msg.type === "youtube-videos" && "videos" in msg) {
            setVideos(msg.videos);
            // A pasted link gives just that video: take it.
            if (isLink && msg.videos.length === 1) setPicked(msg.videos[0].id);
          } else if ("message" in msg) {
            setError(msg.message);
          } else {
            setError("YouTube search failed");
          }
        },
        30000,
      );
    },
    [requestReply],
  );

  // A new search replaces the list, and with it the choice: one made in the old list could
  // be applied without being shown.
  const search = (q: string) => {
    const trimmed = q.trim();
    if (!trimmed) return;
    setIsSearching(true);
    setError(null);
    setVideos(null);
    setPicked(track?.yt_id ?? "");
    request(trimmed);
  };

  // Start over, and search right away, for every track the dialog is opened for.
  const [openedFor, setOpenedFor] = useState<ITrack | null>(null);
  if (track !== openedFor) {
    setOpenedFor(track);
    if (track) {
      setQuery(defaultQuery(track));
      setVideos(null);
      setError(null);
      setPicked(track.yt_id);
      setIsSearching(!!defaultQuery(track).trim());
    }
  }
  useEffect(() => {
    const q = track ? defaultQuery(track).trim() : "";
    if (q) request(q);
    // Closed: drop the reply to a search still under way.
    else searchId.current++;
  }, [track, request]);

  const commit = () => {
    if (track && picked && picked !== track.yt_id) onChange(track, picked);
    onClose();
  };

  return (
    <Dialog
      isOpen={track !== null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      purpose="form"
      width={640}
    >
      <Layout
        header={
          <DialogHeader
            title="Change YouTube video"
            subtitle={
              track
                ? `${track.artists.join(", ")} – ${track.title}${track.mix_name ? ` (${track.mix_name})` : ""}`
                : undefined
            }
            onOpenChange={onClose}
          />
        }
        content={
          <LayoutContent>
            <VStack gap={3}>
              <HStack gap={2} vAlign="end">
                <StackItem size="fill">
                  <TextInput
                    label="Search YouTube or paste a video link"
                    type="search"
                    enterKeyHint="search"
                    hasClear
                    value={query}
                    onChange={setQuery}
                    onKeyDown={(ev) => {
                      if (ev.key === "Enter") search(query);
                    }}
                  />
                </StackItem>
                <Button
                  label="Search"
                  variant="secondary"
                  isDisabled={!query.trim()}
                  onClick={() => search(query)}
                />
              </HStack>
              {error && <Banner status="error" title={error} />}
              {isSearching && !videos ? (
                <HStack hAlign="center">
                  <Spinner label="Searching YouTube…" />
                </HStack>
              ) : videos?.length === 0 ? (
                <EmptyState
                  isCompact
                  title="No videos found"
                  description="Try another search, or paste a video link."
                />
              ) : videos ? (
                <RadioList
                  label="Videos"
                  isLabelHidden
                  value={picked}
                  onChange={setPicked}
                >
                  {videos.map((video) => (
                    <RadioListItem
                      key={video.id}
                      value={video.id}
                      label={
                        (video.title || video.id) +
                        (video.id === track?.yt_id ? " (current)" : "")
                      }
                      description={describe(video)}
                      startContent={
                        // Not Astryx's Thumbnail: that is a square preview of an image
                        // attachment, without lazy loading. This is 4:3, as YouTube's
                        // thumbnails and the grid's track thumbnails are, and loads lazily
                        // (a search lists up to 20).
                        <img
                          className="video-thumbnail"
                          src={`https://i.ytimg.com/vi/${video.id}/default.jpg`}
                          alt=""
                          loading="lazy"
                        />
                      }
                      endContent={
                        <IconButton
                          variant="ghost"
                          size="sm"
                          label={`Open ${video.title || video.id} in YouTube`}
                          tooltip="Open in YouTube"
                          icon={<YouTubeIcon size={15} />}
                          href={`https://www.youtube.com/watch?v=${video.id}`}
                          target="_blank"
                        />
                      }
                    />
                  ))}
                </RadioList>
              ) : null}
            </VStack>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              <Button label="Cancel" variant="secondary" onClick={onClose} />
              <Button
                label="Use this video"
                variant="primary"
                isDisabled={!picked || picked === track?.yt_id || isSearching}
                onClick={commit}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}
