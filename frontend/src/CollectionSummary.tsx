import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent } from "@astryxdesign/core/Layout";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { List, ListItem } from "@astryxdesign/core/List";
import { Button } from "@astryxdesign/core/Button";
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
  onClick,
}: {
  title: string;
  names: IScoredName[];
  onClick: (name: string) => void;
}) {
  if (names.length === 0) {
    return null;
  }

  return (
    <VStack gap={2} width="100%">
      <Text as="div" type="label" weight="bold" color="secondary">
        {title}
      </Text>
      <List hasDividers density="compact">
        {names.map((n) => (
          <ListItem
            key={n.name}
            label={n.name}
            description={`${n.count} tracks · weighted average release age: ${Math.round(n.weighted_avg_days_since_release)} days`}
            endContent={
              <Button
                label={`Search ${n.name}`}
                variant="ghost"
                size="sm"
                onClick={() => onClick(n.name)}
              />
            }
          />
        ))}
      </List>
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
                onClick={handleArtistClick}
              />
              <ScoredNameColumn
                title="Top artists (4★+)"
                names={stats.top_artists_four_plus}
                onClick={handleArtistClick}
              />
              <ScoredNameColumn
                title="Top labels (5★)"
                names={stats.top_labels_five_star}
                onClick={handleLabelClick}
              />
              <ScoredNameColumn
                title="Top labels (4★+)"
                names={stats.top_labels_four_plus}
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
