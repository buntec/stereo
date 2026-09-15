import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent } from "@astryxdesign/core/Layout";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { Token } from "@astryxdesign/core/Token";
import type { IScoredName, ICollectionStats } from "./Types.tsx";

type Props = {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  stats: ICollectionStats;
  totalTracks?: number;
  onArtistClick: (name: string) => void;
  onLabelClick: (name: string) => void;
};

function ScoredNameColumn({
  title,
  names,
  color,
  onClick,
}: {
  title: string;
  names: IScoredName[];
  color: "blue" | "purple";
  onClick: (name: string) => void;
}) {
  if (names.length === 0) {
    return null;
  }

  return (
    <VStack gap={2} className="stats-column">
      <Text as="div" type="label" weight="bold" color="secondary">
        {title}
      </Text>
      <VStack gap={1} hAlign="start">
        {names.map((n) => (
          <Token
            key={n.name}
            color={color}
            label={`${n.name} (${n.count}, ${Math.round(n.weighted_avg_days_since_release)}d)`}
            onClick={() => onClick(n.name)}
          />
        ))}
      </VStack>
    </VStack>
  );
}

function CollectionSummary({
  isOpen,
  onOpenChange,
  stats,
  totalTracks,
  onArtistClick,
  onLabelClick,
}: Props) {
  const handleArtistClick = (name: string) => {
    onOpenChange(false);
    onArtistClick(name);
  };

  const handleLabelClick = (name: string) => {
    onOpenChange(false);
    onLabelClick(name);
  };

  return (
    <Dialog isOpen={isOpen} onOpenChange={onOpenChange} width={640}>
      <Layout
        header={
          <DialogHeader
            title="Library stats"
            onOpenChange={() => onOpenChange(false)}
            endContent={
              totalTracks !== undefined ? (
                <Text type="supporting">{totalTracks} tracks total</Text>
              ) : undefined
            }
          />
        }
        content={
          <LayoutContent>
            <HStack gap={6} wrap="wrap">
              <ScoredNameColumn
                title="Top artists (5★)"
                names={stats.top_artists_five_star}
                color="blue"
                onClick={handleArtistClick}
              />
              <ScoredNameColumn
                title="Top artists (4★+)"
                names={stats.top_artists_four_plus}
                color="blue"
                onClick={handleArtistClick}
              />
              <ScoredNameColumn
                title="Top labels (5★)"
                names={stats.top_labels_five_star}
                color="purple"
                onClick={handleLabelClick}
              />
              <ScoredNameColumn
                title="Top labels (4★+)"
                names={stats.top_labels_four_plus}
                color="purple"
                onClick={handleLabelClick}
              />
            </HStack>
          </LayoutContent>
        }
      />
    </Dialog>
  );
}

export default CollectionSummary;
