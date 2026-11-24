import { useReducer } from "react";
import { useHoverCard } from "@astryxdesign/core/HoverCard";
import { Button } from "@astryxdesign/core/Button";
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

const ago = (iso: string) => timeAgo(Date.parse(iso));

/** The version number as a button, with an icon at its end for whether it's the
 * latest release. Hovering, focusing, tapping or pressing it shows the details and a link to the release
 * page, which the desktop app opens in the browser. A HoverCard rather than a
 * Tooltip, since the card holds a link and a tap on touch screens must open it.
 * The hook rather than the HoverCard component, for its `show()`: pressing the
 * button (Enter, Space, a click) opens the card too, as a button should. A
 * controlled `isOpen` can't do that, since a held `false` also blocks hover. */
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
  let icon: React.ReactElement<React.ComponentProps<typeof Icon>>;
  let url = RELEASES_URL;
  let linkText = "All releases";
  const footer: string[] = [];
  switch (status.state) {
    case "available":
    case "up-to-date": {
      const { latest, refresh_error } = status;
      url = releaseUrl(latest.url);
      footer.push(`Checked ${ago(status.checked_at)}`);
      if (refresh_error)
        footer.push(
          `Checking again failed ${ago(refresh_error.failed_at)}: ${refresh_error.message}`,
        );
      if (status.state === "available") {
        const released = releaseDate(latest.published_at);
        label = `Stereo v${latest.version} is available`;
        lines = [
          `You're running v${version}.`,
          ...(released
            ? [`v${latest.version} was released on ${released}.`]
            : []),
        ];
        // Still true when a later check failed: there's no going back.
        icon = <Icon icon={CircleArrowUp} color="info" size="sm" />;
        linkText = `What's new in v${latest.version}`;
      } else {
        // The backend compares versions; the wording leaves it at that, since
        // equal versions may be spelled differently (`0.2` and `0.2.0`).
        lines = [`You're running v${version}.`];
        if (refresh_error?.is_stale) {
          // Maybe out of date by now.
          label = "Couldn't check for updates";
          lines.push(`The latest release was v${latest.version} when checked.`);
          icon = <Icon icon="warning" color="tertiary" size="sm" />;
        } else {
          label = "Stereo is up to date";
          lines.push(`The latest release is v${latest.version}.`);
          icon = <Icon icon="success" color="success" size="sm" />;
        }
        linkText = `Release notes for v${latest.version}`;
      }
      break;
    }
    case "error":
      label = "Couldn't check for updates";
      lines = [status.message];
      footer.push(`Tried ${ago(status.checked_at)}`);
      icon = <Icon icon="warning" color="tertiary" size="sm" />;
      break;
  }
  const {
    ref: triggerRef,
    isOpen,
    id: cardId,
    show,
    renderHoverCard,
  } = useHoverCard({
    label,
    placement: "below",
    touchTrigger: "tap",
    onShow: refresh,
  });
  return (
    <>
      <Button
        ref={triggerRef}
        // The visible text leads the accessible name.
        label={`v${version}, ${label}`}
        endContent={icon}
        variant="ghost"
        size="sm"
        aria-haspopup="dialog"
        aria-expanded={isOpen}
        aria-controls={isOpen ? cardId : undefined}
        onClick={show}
      >
        <Text type="supporting" color="secondary">
          v{version}
        </Text>
      </Button>
      {renderHoverCard(
        <VStack gap={1}>
          <Text type="supporting" color="primary" weight="semibold">
            {label}
          </Text>
          {lines.map((line) => (
            <Text key={line} type="supporting" color="primary">
              {line}
            </Text>
          ))}
          {footer.map((line) => (
            <Text key={line} type="supporting">
              {line}
            </Text>
          ))}
          <Link href={url} type="supporting" isExternalLink>
            {linkText}
          </Link>
        </VStack>,
      )}
    </>
  );
}

export default UpdateIndicator;
