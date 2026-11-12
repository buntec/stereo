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
  extra,
}: {
  title: string;
  artists?: string[];
  trackName?: string;
  label?: string;
  releaseDate?: string;
  onArtistClick?: (name: string) => void;
  onLabelClick?: (name: string) => void;
  /** Shown after the other metadata, e.g. the measured tempo. */
  extra?: ReactNode;
}) {
  const textType = "supporting";
  const searchable = (name: string, onClick?: (name: string) => void) =>
    onClick ? (
      <Button
        label={name}
        variant="ghost"
        size="sm"
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
  if (extra) groups.push(extra);

  return (
    <VStack gap={0} width="100%" hAlign="start" className="now-playing-title">
      <Text as="h2" type="body" weight="bold" maxLines={1}>
        {trackName ?? title}
      </Text>
      {groups.length > 0 && (
        <HStack
          gap={0}
          wrap="wrap"
          vAlign="center"
          hAlign="start"
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
