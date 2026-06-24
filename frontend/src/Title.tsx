import React from "react";
import { Box, Text } from "@radix-ui/themes";

function Title({
  title,
  artists,
  trackName,
  label,
  releaseDate,
  onArtistClick,
  onLabelClick,
}: {
  title: string;
  artists?: string[];
  trackName?: string;
  label?: string;
  releaseDate?: string;
  onArtistClick?: (name: string) => void;
  onLabelClick?: (name: string) => void;
}) {
  const parts: React.ReactNode[] = [];

  if (artists && artists.length > 0) {
    artists.forEach((artist, i) => {
      if (i > 0) parts.push(", ");
      if (onArtistClick) {
        parts.push(
          <span
            key={`artist-${i}`}
            onClick={() => onArtistClick(artist)}
            className="title-clickable"
          >
            {artist}
          </span>,
        );
      } else {
        parts.push(artist);
      }
    });
  }

  if (label) {
    if (parts.length > 0) parts.push(" - ");
    if (onLabelClick) {
      parts.push(
        <span
          key="label"
          onClick={() => onLabelClick(label)}
          className="title-clickable"
        >
          {label}
        </span>,
      );
    } else {
      parts.push(label);
    }
  }

  if (releaseDate) {
    if (parts.length > 0) parts.push(" - ");
    parts.push(releaseDate);
  }

  return (
    <Box className="title" style={{ textAlign: "center" }}>
      <Text size="7" weight="light" as="div">
        {trackName ?? title}
      </Text>
      {parts.length > 0 && (
        <Text size="4" color="gray" as="div">
          {parts}
        </Text>
      )}
    </Box>
  );
}

export default Title;
