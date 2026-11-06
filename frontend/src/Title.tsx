import { Fragment, type ReactNode } from "react";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { Button } from "@astryxdesign/core/Button";

export default function Title({
  title,
  artists,
  trackName,
  label,
  releaseDate,
  onArtistClick,
  onLabelClick,
  expanded = false,
}: {
  title: string;
  artists?: string[];
  trackName?: string;
  label?: string;
  releaseDate?: string;
  onArtistClick?: (name: string) => void;
  onLabelClick?: (name: string) => void;
  // Large, centered presentation (fullscreen) instead of the compact footer one.
  expanded?: boolean;
}) {
  const textType = expanded ? "body" : "supporting";
  const searchable = (name: string, onClick?: (name: string) => void) =>
    onClick ? (
      <Button
        label={name}
        variant="ghost"
        size={expanded ? "md" : "sm"}
        tooltip={`Search for ${name}`}
        onClick={() => onClick(name)}
      />
    ) : (
      <Text type={textType}>{name}</Text>
    );

  const groups: ReactNode[] = [];
  if (artists && artists.length > 0) {
    groups.push(
      artists.map((artist, index) => (
        <Fragment key={`artist-${index}`}>
          {index > 0 && <Text type={textType}>,&nbsp;</Text>}
          {searchable(artist, onArtistClick)}
        </Fragment>
      )),
    );
  }
  if (label) groups.push(searchable(label, onLabelClick));
  if (releaseDate) groups.push(<Text type={textType}>{releaseDate}</Text>);

  return (
    <VStack
      gap={0}
      width="100%"
      hAlign={expanded ? "center" : "start"}
      className="now-playing-title"
    >
      <Text
        as="h2"
        type={expanded ? "display-2" : "body"}
        weight={expanded ? "normal" : "bold"}
        justify={expanded ? "center" : "start"}
        maxLines={expanded ? 2 : 1}
      >
        {trackName ?? title}
      </Text>
      {groups.length > 0 && (
        <HStack
          gap={0}
          wrap="wrap"
          vAlign="center"
          hAlign={expanded ? "center" : "start"}
          className="now-playing-meta"
        >
          {groups.map((group, index) => (
            <Fragment key={index}>
              {index > 0 && <Text type={textType}>&nbsp;·&nbsp;</Text>}
              {group}
            </Fragment>
          ))}
        </HStack>
      )}
    </VStack>
  );
}
