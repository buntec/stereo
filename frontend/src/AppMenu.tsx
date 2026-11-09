import { useState, type ReactNode } from "react";
import {
  DropdownMenu,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
} from "@astryxdesign/core/DropdownMenu";
import { Divider } from "@astryxdesign/core/Divider";
import { HStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import {
  DesktopIcon,
  DiscIcon,
  GearIcon,
  MoonIcon,
  SunIcon,
} from "@radix-ui/react-icons";
import { MenuCheck, MenuHeading } from "./Menu.tsx";
import SpotifyIcon from "./SpotifyIcon.tsx";
import YouTubeIcon from "./YouTubeIcon.tsx";
import SpotifySessionDialog from "./SpotifySessionDialog.tsx";
import BeatportSessionDialog from "./BeatportSessionDialog.tsx";
import YouTubeSessionDialog from "./YouTubeSessionDialog.tsx";
import type { AppearanceType, RequestReply } from "./Types.tsx";

type AppMenuProps = {
  version?: string;
  appearance: AppearanceType;
  setAppearance: (appearance: AppearanceType) => void;
  requestReply: RequestReply;
  youtubeConnected: boolean;
  youtubeClientId: string | null;
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

function connectionLabel(connected: boolean) {
  return connected ? "Connected" : "Not connected";
}

function AppMenu({
  version,
  appearance,
  setAppearance,
  requestReply,
  youtubeConnected,
  youtubeClientId,
  collectionOpen,
  spotifyBackfillRunning,
  spotifyBackfillChecked,
  spotifyBackfillMatched,
  spotifyBackfillTotal,
  onSpotifyBackfill,
}: AppMenuProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [spotifyConnected, setSpotifyConnected] = useState(false);
  const [spotifyOpen, setSpotifyOpen] = useState(false);
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

  const openDialog = (open: (v: boolean) => void) => {
    setDialogKey((k) => k + 1);
    open(true);
  };

  return (
    <>
      {/* DropdownMenu only fires onOpenChange when controlled. */}
      <DropdownMenu
        hasChevron={false}
        isMenuOpen={menuOpen}
        onOpenChange={(open) => {
          if (open) {
            refreshSpotifySession();
            refreshBeatportSession();
          }
          setMenuOpen(open);
        }}
        menuWidth={240}
        button={{
          label: "Settings",
          icon: <GearIcon />,
          isIconOnly: true,
          variant: "ghost",
          tooltip: "Settings",
        }}
      >
        <MenuHeading>Accounts</MenuHeading>
        <DropdownMenuItem
          icon={<SpotifyIcon />}
          label="Spotify"
          description={connectionLabel(spotifyConnected)}
          onClick={() => openDialog(setSpotifyOpen)}
        />
        <DropdownMenuItem
          icon={<YouTubeIcon />}
          label="YouTube"
          description={connectionLabel(youtubeConnected)}
          onClick={() => openDialog(setYoutubeOpen)}
        />
        <DropdownMenuItem
          icon={<DiscIcon />}
          label="Beatport"
          description={connectionLabel(beatportBrowser !== null)}
          onClick={() => openDialog(setBeatportOpen)}
        />

        <Divider />

        <MenuHeading>Appearance</MenuHeading>
        <DropdownMenuRadioGroup
          aria-label="Appearance"
          value={appearance}
          onChange={(v) => setAppearance(v as AppearanceType)}
        >
          {appearanceOptions.map(({ value, label, icon }) => (
            <DropdownMenuRadioItem
              key={value}
              value={value}
              label={label}
              icon={icon}
              endContent={<MenuCheck isChecked={appearance === value} />}
            />
          ))}
        </DropdownMenuRadioGroup>

        <Divider />

        <HStack paddingInline={2} paddingBlock={1}>
          <Text type="supporting" color="secondary">
            Stereo {version}
          </Text>
        </HStack>
      </DropdownMenu>

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
        requestReply={requestReply}
        connected={youtubeConnected}
        clientId={youtubeClientId}
      />
    </>
  );
}

export default AppMenu;
