import { IconButton } from "@astryxdesign/core/IconButton";
import { Icon } from "@astryxdesign/core/Icon";
import { Text } from "@astryxdesign/core/Text";
import { Tooltip } from "@astryxdesign/core/Tooltip";
import { VStack } from "@astryxdesign/core/Stack";
import { CircleArrowUp } from "lucide-react";
import type { UpdateStatus } from "./Types.tsx";
import { timeAgo } from "./Utils.tsx";

const RELEASES_URL = "https://github.com/buntec/stereo/releases";

const releaseDate = (iso?: string | null) =>
  iso ? new Date(iso).toLocaleDateString() : undefined;

/** An icon beside the version number: whether it's the latest release, with
 * the details on hover. Links to the release page, which the desktop app opens
 * in the browser. */
function UpdateIndicator({
  version,
  status,
}: {
  version: string;
  status: UpdateStatus;
}) {
  const checked = `Checked ${timeAgo(Date.parse(status.checked_at))}`;
  let label: string;
  let lines: string[];
  let icon;
  let url = RELEASES_URL;
  switch (status.state) {
    case "available": {
      const released = releaseDate(status.latest.published_at);
      label = `Stereo v${status.latest.version} is available`;
      lines = [
        `You're running v${version}.${released ? ` v${status.latest.version} was released on ${released}.` : ""}`,
        "Click to open the release page.",
      ];
      icon = <Icon icon={CircleArrowUp} color="info" size="sm" />;
      url = status.latest.url;
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
      url = status.latest.url;
      break;
    case "error":
      label = "Couldn't check for updates";
      lines = [status.message];
      icon = <Icon icon="warning" color="tertiary" size="sm" />;
      break;
  }
  return (
    <Tooltip
      placement="below"
      content={
        <VStack gap={0.5}>
          <Text type="inherit" color="inherit" weight="semibold">
            {label}
          </Text>
          {lines.map((line) => (
            <Text key={line} type="inherit" color="inherit">
              {line}
            </Text>
          ))}
          <Text type="inherit" color="inherit">
            {checked}
          </Text>
        </VStack>
      }
    >
      <IconButton
        label={label}
        icon={icon}
        variant="ghost"
        size="sm"
        href={url}
        target="_blank"
        rel="noopener noreferrer"
      />
    </Tooltip>
  );
}

export default UpdateIndicator;
