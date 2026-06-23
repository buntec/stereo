import { Flex, Text, Badge } from "@radix-ui/themes";
import type { ICollectionStats } from "./Types.tsx";

type Props = {
  stats: ICollectionStats;
  onArtistClick: (name: string) => void;
  onLabelClick: (name: string) => void;
};

function CollectionSummary({ stats, onArtistClick, onLabelClick }: Props) {
  if (stats.top_artists.length === 0 && stats.top_labels.length === 0) {
    return null;
  }

  return (
    <Flex className="collection-summary" direction="row" gap="4">
      {stats.top_artists.length > 0 && (
        <Flex direction="column" gap="1">
          <Text size="1" color="gray" weight="bold">
            Top artists
          </Text>
          {stats.top_artists.map((a) => (
            <Badge
              key={a.name}
              variant="soft"
              color="blue"
              style={{ cursor: "pointer", width: "fit-content" }}
              onClick={() => onArtistClick(a.name)}
            >
              {a.name}
            </Badge>
          ))}
        </Flex>
      )}
      {stats.top_labels.length > 0 && (
        <Flex direction="column" gap="1">
          <Text size="1" color="gray" weight="bold">
            Top labels
          </Text>
          {stats.top_labels.map((l) => (
            <Badge
              key={l.name}
              variant="soft"
              color="purple"
              style={{ cursor: "pointer", width: "fit-content" }}
              onClick={() => onLabelClick(l.name)}
            >
              {l.name}
            </Badge>
          ))}
        </Flex>
      )}
    </Flex>
  );
}

export default CollectionSummary;
