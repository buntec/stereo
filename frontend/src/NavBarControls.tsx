import { useEffect, useState, type ReactNode } from "react";
import { IconButton } from "@astryxdesign/core/IconButton";
import {
  SegmentedControl,
  SegmentedControlItem,
} from "@astryxdesign/core/SegmentedControl";
import { HStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { DesktopIcon, MoonIcon, SunIcon } from "@radix-ui/react-icons";
import SpotifyIcon from "./SpotifyIcon.tsx";
import YouTubeIcon from "./YouTubeIcon.tsx";
import BeatportIcon from "./BeatportIcon.tsx";
import SpotifySessionDialog from "./SpotifySessionDialog.tsx";
import SettingsMenu from "./SettingsMenu.tsx";
import BeatportSessionDialog from "./BeatportSessionDialog.tsx";
import YouTubeSessionDialog from "./YouTubeSessionDialog.tsx";
import {
  youTubeConnection,
  type AppearanceType,
  type RequestReply,
  type YouTubeConnection,
} from "./Types.tsx";

type NavBarControlsProps = {
  version?: string;
  appearance: AppearanceType;
  setAppearance: (appearance: AppearanceType) => void;
  backgroundShown: boolean;
  backgroundPulse: number;
  setBackgroundPulse: (pulse: number) => void;
  isConnected: boolean;
  requestReply: RequestReply;
  collectionOpen: boolean;
  spotifyBackfillRunning: boolean;
  spotifyBackfillChecked?: number;
  spotifyBackfillMatched?: number;
  spotifyBackfillTotal?: number;
  onSpotifyBackfill: () => void;
};

const appearanceOptions: {
  value: AppearanceType;
  label: string;
  icon: ReactNode;
}[] = [
  { value: "system", label: "System", icon: <DesktopIcon /> },
  { value: "light", label: "Light", icon: <SunIcon /> },
  { value: "dark", label: "Dark", icon: <MoonIcon /> },
];

type AccountButtonProps = {
  service: "youtube" | "spotify" | "beatport";
  label: string;
  icon: ReactNode;
  connected: boolean;
  onClick: () => void;
};

// The icon shows the service's brand color when connected and is muted otherwise
// (see .account-button in App.css).
function AccountButton({
  service,
  label,
  icon,
  connected,
  onClick,
}: AccountButtonProps) {
  const status = connected ? "Connected" : "Not connected";
  return (
    <IconButton
      className={`account-button account-${service}`}
      data-connected={connected}
      label={`${label}: ${status}`}
      tooltip={`${label}: ${status}`}
      icon={icon}
      variant="ghost"
      onClick={onClick}
    />
  );
}

function NavBarControls({
  version,
  appearance,
  setAppearance,
  backgroundShown,
  backgroundPulse,
  setBackgroundPulse,
  isConnected,
  requestReply,
  collectionOpen,
  spotifyBackfillRunning,
  spotifyBackfillChecked,
  spotifyBackfillMatched,
  spotifyBackfillTotal,
  onSpotifyBackfill,
}: NavBarControlsProps) {
  const [spotifyConnected, setSpotifyConnected] = useState(false);
  const [spotifyOpen, setSpotifyOpen] = useState(false);
  const [youtubeConnection, setYoutubeConnection] =
    useState<YouTubeConnection | null>(null);
  const [youtubeOpen, setYoutubeOpen] = useState(false);
  const [beatportBrowser, setBeatportBrowser] = useState<string | null>(null);
  const [beatportOpen, setBeatportOpen] = useState(false);
  // Remount the account dialogs on every opening so they start from clean
  // inputs.
  const [dialogKey, setDialogKey] = useState(0);

  const refreshSpotifySession = () =>
    requestReply({ type: "get-spotify-session" }, (r) => {
      if ("connected" in r) setSpotifyConnected(r.connected);
    });

  const refreshBeatportSession = () =>
    requestReply({ type: "get-beatport-session" }, (r) => {
      if (r.type === "beatport-session" && "browser" in r)
        setBeatportBrowser(r.browser);
    });

  const refreshYouTubeSession = () =>
    requestReply({ type: "get-youtube-session" }, (r) => {
      if (r.type === "youtube-session" && "browser" in r)
        setYoutubeConnection(youTubeConnection(r));
    });

  // The account buttons show connection status at all times, so fetch it as soon
  // as the socket is (re)connected.
  useEffect(() => {
    if (!isConnected) return;
    refreshSpotifySession();
    refreshBeatportSession();
    refreshYouTubeSession();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isConnected, requestReply]);

  const openDialog = (open: (v: boolean) => void, refresh: () => void) => {
    refresh();
    setDialogKey((k) => k + 1);
    open(true);
  };

  return (
    <>
      <HStack vAlign="center" gap={2}>
        {version && (
          <Text type="supporting" color="secondary">
            v{version}
          </Text>
        )}
        <HStack vAlign="center">
          <AccountButton
            service="youtube"
            label="YouTube"
            icon={<YouTubeIcon size={16} />}
            connected={youtubeConnection !== null}
            onClick={() => openDialog(setYoutubeOpen, refreshYouTubeSession)}
          />
          <AccountButton
            service="spotify"
            label="Spotify"
            icon={<SpotifyIcon size={16} />}
            connected={spotifyConnected}
            onClick={() => openDialog(setSpotifyOpen, refreshSpotifySession)}
          />
          <AccountButton
            service="beatport"
            label="Beatport"
            icon={<BeatportIcon size={16} />}
            connected={beatportBrowser !== null}
            onClick={() => openDialog(setBeatportOpen, refreshBeatportSession)}
          />
        </HStack>
        <SegmentedControl
          label="Appearance"
          size="sm"
          value={appearance}
          onChange={(v) => setAppearance(v as AppearanceType)}
        >
          {appearanceOptions.map(({ value, label, icon }) => (
            <SegmentedControlItem
              key={value}
              value={value}
              label={label}
              icon={icon}
              isLabelHidden
            />
          ))}
        </SegmentedControl>
        <SettingsMenu
          backgroundShown={backgroundShown}
          backgroundPulse={backgroundPulse}
          setBackgroundPulse={setBackgroundPulse}
        />
      </HStack>

      <SpotifySessionDialog
        key={`spotify-${dialogKey}`}
        isOpen={spotifyOpen}
        onOpenChange={setSpotifyOpen}
        connected={spotifyConnected}
        setConnected={setSpotifyConnected}
        requestReply={requestReply}
        collectionOpen={collectionOpen}
        backfillRunning={spotifyBackfillRunning}
        backfillChecked={spotifyBackfillChecked}
        backfillMatched={spotifyBackfillMatched}
        backfillTotal={spotifyBackfillTotal}
        onBackfill={onSpotifyBackfill}
      />
      <BeatportSessionDialog
        key={`beatport-${dialogKey}`}
        isOpen={beatportOpen}
        onOpenChange={setBeatportOpen}
        browser={beatportBrowser}
        setBrowser={setBeatportBrowser}
        requestReply={requestReply}
      />
      <YouTubeSessionDialog
        key={`youtube-${dialogKey}`}
        isOpen={youtubeOpen}
        onOpenChange={setYoutubeOpen}
        connection={youtubeConnection}
        setConnection={setYoutubeConnection}
        requestReply={requestReply}
      />
    </>
  );
}

export default NavBarControls;
