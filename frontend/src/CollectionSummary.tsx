import { useState } from "react";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent } from "@astryxdesign/core/Layout";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { Token } from "@astryxdesign/core/Token";
import { IconButton } from "@astryxdesign/core/IconButton";
import { InfoCircledIcon } from "@radix-ui/react-icons";
import type { IScoredName, ICollectionStats } from "./Types.tsx";

type Props = {
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
  stats,
  totalTracks,
  onArtistClick,
  onLabelClick,
}: Props) {
  const [open, setOpen] = useState(false);

  const empty =
    stats.top_artists_five_star.length === 0 &&
    stats.top_artists_four_plus.length === 0 &&
    stats.top_labels_five_star.length === 0 &&
    stats.top_labels_four_plus.length === 0;
  if (empty) {
    return null;
  }

  const handleArtistClick = (name: string) => {
    setOpen(false);
    onArtistClick(name);
  };

  const handleLabelClick = (name: string) => {
    setOpen(false);
    onLabelClick(name);
  };

  return (
    <>
      <IconButton
        variant="ghost"
        size="sm"
        label="Library stats"
        tooltip="Library stats"
        icon={<InfoCircledIcon />}
        onClick={() => setOpen(true)}
      />

      <Dialog isOpen={open} onOpenChange={setOpen} width={640}>
        <Layout
          header={
            <DialogHeader
              title="Library stats"
              onOpenChange={() => setOpen(false)}
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
    </>
  );
}

export default CollectionSummary;
