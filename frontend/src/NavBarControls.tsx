import { useEffect, useState, type ReactNode } from "react";
import { IconButton } from "@astryxdesign/core/IconButton";
import {
  SegmentedControl,
  SegmentedControlItem,
} from "@astryxdesign/core/SegmentedControl";
import { HStack } from "@astryxdesign/core/Stack";
import { StatusDot } from "@astryxdesign/core/StatusDot";
import { Text } from "@astryxdesign/core/Text";
import { DesktopIcon, MoonIcon, SunIcon } from "@radix-ui/react-icons";
import SpotifyIcon from "./SpotifyIcon.tsx";
import YouTubeIcon from "./YouTubeIcon.tsx";
import BeatportIcon from "./BeatportIcon.tsx";
import SpotifySessionDialog from "./SpotifySessionDialog.tsx";
import SettingsMenu from "./SettingsMenu.tsx";
import TailscaleButton from "./TailscaleButton.tsx";
import HealthCheckButton from "./HealthCheckButton.tsx";
import BeatportSessionDialog from "./BeatportSessionDialog.tsx";
import YouTubeSessionDialog from "./YouTubeSessionDialog.tsx";
import { inDesktopApp } from "./Utils.tsx";
import {
  youTubeConnection,
  type AppearanceType,
  type RequestReply,
  type AirSettings,
  type BackgroundSettings,
  type DiscoBallSettings,
  type TailscaleStatus,
  type YouTubeConnection,
} from "./Types.tsx";

type NavBarControlsProps = {
  version?: string;
  appearance: AppearanceType;
  setAppearance: (appearance: AppearanceType) => void;
  backgroundShown: boolean;
  backgroundPulse: number;
  setBackgroundPulse: (pulse: number) => void;
  background: BackgroundSettings;
  setBackground: (background: BackgroundSettings) => void;
  air: AirSettings;
  setAir: (air: AirSettings) => void;
  discoBall: DiscoBallSettings;
  setDiscoBall: (discoBall: DiscoBallSettings) => void;
  isConnected: boolean;
  requestReply: RequestReply;
  tailscale?: TailscaleStatus;
  collectionOpen: boolean;
  spotifyBackfillRunning: boolean;
  spotifyBackfillChecked?: number;
  spotifyBackfillMatched?: number;
  spotifyBackfillTotal?: number;
  onSpotifyBackfill: () => void;
  // Narrow screens: the version and the appearance switch move into the settings
  // popover, leaving the row to the collection selector.
  isCompact?: boolean;
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
  label: string;
  icon: ReactNode;
  connected: boolean;
  onClick: () => void;
};

// A monochrome service icon with a StatusDot in the corner for the connection state
// (positioned by .account-button in App.css). The button's label and tooltip spell
// the state out, so it isn't carried by the dot's color alone.
function AccountButton({
  label,
  icon,
  connected,
  onClick,
}: AccountButtonProps) {
  const status = connected ? "Connected" : "Not connected";
  return (
    <IconButton
      className="account-button"
      label={`${label}: ${status}`}
      tooltip={`${label}: ${status}`}
      icon={
        <>
          {icon}
          <StatusDot
            variant={connected ? "success" : "neutral"}
            label={status}
          />
        </>
      }
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
  background,
  setBackground,
  air,
  setAir,
  discoBall,
  setDiscoBall,
  isConnected,
  requestReply,
  tailscale,
  collectionOpen,
  spotifyBackfillRunning,
  spotifyBackfillChecked,
  spotifyBackfillMatched,
  spotifyBackfillTotal,
  onSpotifyBackfill,
  isCompact = false,
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

  // In the desktop app, Beatport is connected from Stereo in a browser: show
  // what was connected there on coming back.
  useEffect(() => {
    if (!inDesktopApp || !isConnected) return;
    const refresh = () => refreshBeatportSession();
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isConnected, requestReply]);

  const openDialog = (open: (v: boolean) => void, refresh: () => void) => {
    refresh();
    setDialogKey((k) => k + 1);
    open(true);
  };

  const versionText = (
    <Text type="supporting" color="secondary">
      v{version}
    </Text>
  );
  const appearanceControl = (
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
  );

  return (
    <>
      <HStack vAlign="center" gap={isCompact ? 1 : 2}>
        {version && !isCompact && versionText}
        <HStack vAlign="center">
          <AccountButton
            label="YouTube"
            icon={<YouTubeIcon size={16} />}
            connected={youtubeConnection !== null}
            onClick={() => openDialog(setYoutubeOpen, refreshYouTubeSession)}
          />
          <AccountButton
            label="Spotify"
            icon={<SpotifyIcon size={16} />}
            connected={spotifyConnected}
            onClick={() => openDialog(setSpotifyOpen, refreshSpotifySession)}
          />
          <AccountButton
            label="Beatport"
            icon={<BeatportIcon size={16} />}
            connected={beatportBrowser !== null}
            onClick={() => openDialog(setBeatportOpen, refreshBeatportSession)}
          />
          <TailscaleButton status={tailscale} requestReply={requestReply} />
        </HStack>
        {!isCompact && appearanceControl}
        <HealthCheckButton
          isConnected={isConnected}
          requestReply={requestReply}
          version={version}
        />
        <SettingsMenu
          backgroundShown={backgroundShown}
          backgroundPulse={backgroundPulse}
          setBackgroundPulse={setBackgroundPulse}
          background={background}
          setBackground={setBackground}
          air={air}
          setAir={setAir}
          discoBall={discoBall}
          setDiscoBall={setDiscoBall}
          header={isCompact ? appearanceControl : undefined}
          footer={isCompact && version ? versionText : undefined}
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
