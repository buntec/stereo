import { useReducer, useState } from "react";
import { useHoverCard } from "@astryxdesign/core/HoverCard";
import { Button } from "@astryxdesign/core/Button";
import { Icon } from "@astryxdesign/core/Icon";
import { Link } from "@astryxdesign/core/Link";
import { Text } from "@astryxdesign/core/Text";
import { VisuallyHidden } from "@astryxdesign/core/VisuallyHidden";
import { HStack, VStack } from "@astryxdesign/core/Stack";
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
 * controlled `isOpen` can't do that, since a held `false` also blocks hover.
 * "Check now" asks the backend to check right away rather than wait for the
 * daily check. */
function UpdateIndicator({
  version,
  status,
  checkForUpdates,
}: {
  version: string;
  status: UpdateStatus;
  /** `done` gets an error message if the check couldn't be made. */
  checkForUpdates: (done: (error?: string) => void) => void;
}) {
  const [checking, setChecking] = useState(false);
  // A failed request; a failed check is reported in `status` instead.
  const [checkError, setCheckError] = useState<string>();
  // Whether the live region should announce the outcome of "Check now", which
  // otherwise appears away from the button that keeps focus.
  const [announce, setAnnounce] = useState(false);
  // A status that arrives anyway (a reply after the timeout, or another
  // session's check) supersedes the failed request.
  const [shownStatus, setShownStatus] = useState(status);
  if (shownStatus !== status) {
    setShownStatus(status);
    setCheckError(undefined);
  }
  const checkNow = () => {
    setChecking(true);
    setCheckError(undefined);
    setAnnounce(false);
    checkForUpdates((error) => {
      setChecking(false);
      setCheckError(error);
      setAnnounce(true);
    });
  };
  // "Checked … ago" is computed on render, and nothing else re-renders this
  // while a tab stays open for hours, so render again on every opening.
  const [, refresh] = useReducer((n: number) => n + 1, 0);
  const onShow = () => {
    setAnnounce(false);
    refresh();
  };
  let label: string;
  let lines: string[];
  let icon: React.ReactElement<React.ComponentProps<typeof Icon>>;
  let url = RELEASES_URL;
  let linkText = "All releases";
  const footer: string[] = [];
  let failure: string | undefined;
  switch (status.state) {
    case "available":
    case "up-to-date": {
      const { latest, refresh_error } = status;
      url = releaseUrl(latest.url);
      footer.push(`Checked ${ago(status.checked_at)}`);
      if (refresh_error) {
        failure = `Checking again failed ${ago(refresh_error.failed_at)}: ${refresh_error.message}`;
        footer.push(failure);
      }
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
      failure = status.message;
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
    onShow,
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
          {checkError && (
            <Text type="supporting" color="secondary">
              Couldn't check: {checkError}
            </Text>
          )}
          {/* Mounted with the card, so it's in place before its text changes. */}
          <VisuallyHidden as="div" role="status" aria-live="polite">
            {announce &&
              (checkError
                ? `Couldn't check: ${checkError}`
                : [label, failure].filter(Boolean).join(". "))}
          </VisuallyHidden>
          <HStack gap={2} vAlign="center" hAlign="between">
            <Link href={url} type="supporting" isExternalLink>
              {linkText}
            </Link>
            <Button
              label="Check now"
              variant="secondary"
              size="sm"
              isLoading={checking}
              onClick={checkNow}
            />
          </HStack>
        </VStack>,
      )}
    </>
  );
}

export default UpdateIndicator;
