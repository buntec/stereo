import { useState } from "react";
import {
  Dialog,
  Flex,
  Text,
  Badge,
  IconButton,
  Tooltip,
} from "@radix-ui/themes";
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
    <Flex direction="column" gap="2" className="stats-column">
      <Text size="1" weight="bold" color="gray">
        {title}
      </Text>
      <Flex direction="column" gap="1" align="start">
        {names.map((n) => (
          <Badge
            key={n.name}
            variant="soft"
            color={color}
            style={{ cursor: "pointer", width: "fit-content" }}
            onClick={() => onClick(n.name)}
          >
            {n.name} ({n.count}, {Math.round(n.weighted_avg_days_since_release)}
            d)
          </Badge>
        ))}
      </Flex>
    </Flex>
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
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Tooltip content="Library stats">
        <Dialog.Trigger>
          <IconButton variant="ghost" size="1" color="gray">
            <InfoCircledIcon />
          </IconButton>
        </Dialog.Trigger>
      </Tooltip>

      <Dialog.Content maxWidth="640px">
        <Flex align="center" justify="between" mb="4">
          <Dialog.Title mb="0">Library stats</Dialog.Title>
          {totalTracks !== undefined && (
            <Text size="2" color="gray">
              {totalTracks} tracks total
            </Text>
          )}
        </Flex>
        <Flex direction="row" gap="6" wrap="wrap">
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
        </Flex>
      </Dialog.Content>
    </Dialog.Root>
  );
}

export default CollectionSummary;
