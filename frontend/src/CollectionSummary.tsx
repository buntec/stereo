import { useState } from "react";
import { Flex, Badge, Select, TextField } from "@radix-ui/themes";
import { StarFilledIcon } from "@radix-ui/react-icons";
import type { IScoredName, ICollectionStats } from "./Types.tsx";

type Props = {
  stats: ICollectionStats;
  onArtistClick: (name: string) => void;
  onLabelClick: (name: string) => void;
};

type RankingMethod = "five_star" | "four_plus";

const RANKING_METHODS: { value: RankingMethod; label: string }[] = [
  { value: "five_star", label: "5 stars" },
  { value: "four_plus", label: "4+ stars" },
];

function ScoredNameColumn({
  names,
  color,
  onClick,
}: {
  names: IScoredName[];
  color: "blue" | "purple";
  onClick: (name: string) => void;
}) {
  if (names.length === 0) {
    return null;
  }

  return (
    <Flex direction="column" gap="1">
      {names.map((n) => (
        <Badge
          key={n.name}
          variant="soft"
          color={color}
          style={{ cursor: "pointer", width: "fit-content" }}
          onClick={() => onClick(n.name)}
        >
          {n.name} ({n.count}, {Math.round(n.avg_days_since_release)}d avg)
        </Badge>
      ))}
    </Flex>
  );
}

function CollectionSummary({ stats, onArtistClick, onLabelClick }: Props) {
  const [method, setMethod] = useState<RankingMethod>("five_star");

  const empty =
    stats.top_artists_five_star.length === 0 &&
    stats.top_artists_four_plus.length === 0 &&
    stats.top_labels_five_star.length === 0 &&
    stats.top_labels_four_plus.length === 0;
  if (empty) {
    return null;
  }

  const fiveStar = method === "five_star";
  const topArtists = fiveStar
    ? stats.top_artists_five_star
    : stats.top_artists_four_plus;
  const topLabels = fiveStar
    ? stats.top_labels_five_star
    : stats.top_labels_four_plus;

  return (
    <Flex className="collection-summary" direction="column" gap="2">
      <div className="rt-TextFieldRoot rt-r-size-1 rt-variant-soft ranking-method-box">
        <TextField.Slot>
          <StarFilledIcon color="gold" height="12" width="12" />
        </TextField.Slot>
        <Select.Root
          size="1"
          value={method}
          onValueChange={(v) => setMethod(v as RankingMethod)}
        >
          <Select.Trigger variant="ghost" />
          <Select.Content>
            {RANKING_METHODS.map((m) => (
              <Select.Item key={m.value} value={m.value}>
                {m.label}
              </Select.Item>
            ))}
          </Select.Content>
        </Select.Root>
      </div>
      <Flex direction="row" gap="4">
        <ScoredNameColumn
          names={topArtists}
          color="blue"
          onClick={onArtistClick}
        />
        <ScoredNameColumn
          names={topLabels}
          color="purple"
          onClick={onLabelClick}
        />
      </Flex>
    </Flex>
  );
}

export default CollectionSummary;
