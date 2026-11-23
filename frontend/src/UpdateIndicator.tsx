import { useReducer } from "react";
import { HoverCard } from "@astryxdesign/core/HoverCard";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Icon } from "@astryxdesign/core/Icon";
import { Link } from "@astryxdesign/core/Link";
import { Text } from "@astryxdesign/core/Text";
import { VStack } from "@astryxdesign/core/Stack";
import { CircleArrowUp } from "lucide-react";
import type { UpdateStatus } from "./Types.tsx";
import { timeAgo } from "./Utils.tsx";

const RELEASES_URL = "https://github.com/buntec/stereo/releases";

// The backend only sends release pages, but the link shouldn't depend on it.
const releaseUrl = (url: string) =>
  url.startsWith(`${RELEASES_URL}/`) ? url : RELEASES_URL;

const releaseDate = (iso?: string | null) =>
  iso ? new Date(iso).toLocaleDateString() : undefined;

/** An icon beside the version number: whether it's the latest release. Hovering
 * or tapping it shows the details and a link to the release page, which the
 * desktop app opens in the browser. A HoverCard rather than a Tooltip, since
 * the card holds a link and a tap on touch screens must open it (the trigger
 * does nothing else). */
function UpdateIndicator({
  version,
  status,
}: {
  version: string;
  status: UpdateStatus;
}) {
  // "Checked … ago" is computed on render, and nothing else re-renders this
  // while a tab stays open for hours, so render again on every opening.
  const [, refresh] = useReducer((n: number) => n + 1, 0);
  let label: string;
  let lines: string[];
  let icon;
  let url = RELEASES_URL;
  let linkText = "All releases";
  switch (status.state) {
    case "available": {
      const released = releaseDate(status.latest.published_at);
      label = `Stereo v${status.latest.version} is available`;
      lines = [
        `You're running v${version}.`,
        ...(released
          ? [`v${status.latest.version} was released on ${released}.`]
          : []),
      ];
      icon = <Icon icon={CircleArrowUp} color="info" size="sm" />;
      url = releaseUrl(status.latest.url);
      linkText = `What's new in v${status.latest.version}`;
      break;
    }
    case "up-to-date":
      label = "Stereo is up to date";
      lines = [
        status.latest.version === version
          ? `v${version} is the latest release.`
          : `v${version} is newer than the latest release, v${status.latest.version}.`,
      ];
      icon = <Icon icon="success" color="success" size="sm" />;
      url = releaseUrl(status.latest.url);
      linkText = `Release notes for v${status.latest.version}`;
      break;
    case "error":
      label = "Couldn't check for updates";
      lines = [status.message];
      icon = <Icon icon="warning" color="tertiary" size="sm" />;
      break;
  }
  return (
    <HoverCard
      label={label}
      placement="below"
      touchTrigger="tap"
      onOpenChange={(open) => open && refresh()}
      content={
        <VStack gap={1}>
          <Text type="supporting" color="primary" weight="semibold">
            {label}
          </Text>
          {lines.map((line) => (
            <Text key={line} type="supporting" color="primary">
              {line}
            </Text>
          ))}
          <Text type="supporting">
            Checked {timeAgo(Date.parse(status.checked_at))}
          </Text>
          <Link href={url} type="supporting" isExternalLink>
            {linkText}
          </Link>
        </VStack>
      }
    >
      <IconButton label={label} icon={icon} variant="ghost" size="sm" />
    </HoverCard>
  );
}

export default UpdateIndicator;
