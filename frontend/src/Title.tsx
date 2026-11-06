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
  expanded?: boolean;
}) {
  return (
    <VStack gap={0} hAlign={expanded ? "center" : "start"}>
      <Text
        as="h2"
        type={expanded ? "display-2" : "body"}
        weight="bold"
        maxLines={expanded ? 2 : 1}
      >
        {trackName ?? title}
      </Text>
      <HStack gap={1} wrap="wrap" vAlign="center">
        {artists?.map((artist) =>
          onArtistClick ? (
            <Button
              key={artist}
              label={artist}
              variant="ghost"
              size="sm"
              onClick={() => onArtistClick(artist)}
            />
          ) : (
            <Text key={artist} type="supporting">
              {artist}
            </Text>
          ),
        )}
        {label &&
          (onLabelClick ? (
            <Button
              label={label}
              variant="ghost"
              size="sm"
              onClick={() => onLabelClick(label)}
            />
          ) : (
            <Text type="supporting">{label}</Text>
          ))}
        {expanded && releaseDate && (
          <Text type="supporting" color="secondary">
            {releaseDate}
          </Text>
        )}
      </HStack>
    </VStack>
  );
}
