import { Fragment, type ReactNode } from "react";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { Link } from "@astryxdesign/core/Link";

export default function Title({
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
  const textType = "supporting";
  const searchable = (name: string, onClick?: (name: string) => void) =>
    onClick ? (
      <Link
        type={textType}
        tooltip={`Search for ${name}`}
        onClick={() => onClick(name)}
      >
        {name}
      </Link>
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
    <VStack gap={0} width="100%" hAlign="start" className="now-playing-title">
      <Text as="h2" type="body" weight="bold" maxLines={1}>
        {trackName ?? title}
      </Text>
      {groups.length > 0 && (
        <HStack gap={0} wrap="wrap" vAlign="center" hAlign="start">
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
