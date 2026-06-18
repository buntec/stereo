import { useState, useEffect, useRef } from "react";
import {
  Text,
  Button,
  Dialog,
  TextField,
  Flex,
  Code,
  Progress,
  Spinner,
} from "@radix-ui/themes";
import { useLocalStorage } from "./Utils.tsx";

// ─── OAuth progress state ─────────────────────────────────────────────────────

export type OAuthPhase =
  | { kind: "idle" }
  | { kind: "exchanging-token" }
  | { kind: "creating-playlist" }
  | { kind: "adding-tracks"; current: number; total: number };

// ─── PKCE helpers ─────────────────────────────────────────────────────────────

function generateCodeVerifier(): string {
  const bytes = new Uint8Array(64);
  crypto.getRandomValues(bytes);
  return btoa(String.fromCharCode(...bytes))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=/g, "");
}

async function generateCodeChallenge(verifier: string): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(verifier),
  );
  return btoa(String.fromCharCode(...new Uint8Array(digest)))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=/g, "");
}

// ─── YouTube API helpers ──────────────────────────────────────────────────────

const YT_TOKEN_URL = "https://oauth2.googleapis.com/token";
const YT_API_BASE = "https://www.googleapis.com/youtube/v3";

async function exchangeToken(
  code: string,
  verifier: string,
  clientId: string,
  clientSecret: string,
): Promise<string> {
  const res = await fetch(YT_TOKEN_URL, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      code,
      client_id: clientId,
      client_secret: clientSecret,
      redirect_uri: window.location.origin,
      grant_type: "authorization_code",
      code_verifier: verifier,
    }),
  });
  if (!res.ok) throw new Error(`Token exchange failed: ${res.status}`);
  const data = await res.json();
  return data.access_token as string;
}

async function createYouTubePlaylist(
  accessToken: string,
  name: string,
): Promise<{ id: string; url: string }> {
  const res = await fetch(`${YT_API_BASE}/playlists?part=snippet,status`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${accessToken}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      snippet: { title: name, description: "Created by stereo" },
      status: { privacyStatus: "private" },
    }),
  });
  if (!res.ok) throw new Error(`Failed to create playlist: ${res.status}`);
  const data = await res.json();
  return {
    id: data.id as string,
    url: `https://www.youtube.com/playlist?list=${data.id}`,
  };
}

async function addVideoToPlaylist(
  accessToken: string,
  playlistId: string,
  videoId: string,
): Promise<void> {
  const res = await fetch(`${YT_API_BASE}/playlistItems?part=snippet`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${accessToken}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      snippet: {
        playlistId,
        resourceId: { kind: "youtube#video", videoId },
      },
    }),
  });
  // 409 = video already in playlist, ignore
  if (!res.ok && res.status !== 409) {
    throw new Error(`Failed to add video ${videoId}: ${res.status}`);
  }
}

// ─── Session storage keys ─────────────────────────────────────────────────────

const SS_VERIFIER = "yt_oauth_verifier";
const SS_YT_IDS = "yt_oauth_yt_ids";
const SS_PLAYLIST_NAME = "yt_oauth_playlist_name";
const SS_CLIENT_ID = "yt_oauth_client_id_pending";
const SS_CLIENT_SECRET = "yt_oauth_client_secret_pending";
const OAUTH_STATE = "stereo-yt-export";

// ─── Hook: handle OAuth callback on app load ──────────────────────────────────

export function useYouTubeOAuthCallback(
  onSuccess: (url: string, count: number) => void,
  onError: (msg: string) => void,
): OAuthPhase {
  const [phase, setPhase] = useState<OAuthPhase>({ kind: "idle" });
  const onSuccessRef = useRef(onSuccess);
  const onErrorRef = useRef(onError);
  onSuccessRef.current = onSuccess;
  onErrorRef.current = onError;

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const code = params.get("code");
    const state = params.get("state");

    if (!code || state !== OAUTH_STATE) return;

    history.replaceState({}, "", window.location.pathname);

    const verifier = sessionStorage.getItem(SS_VERIFIER);
    const ytIdsRaw = sessionStorage.getItem(SS_YT_IDS);
    const playlistName = sessionStorage.getItem(SS_PLAYLIST_NAME);
    const clientId = sessionStorage.getItem(SS_CLIENT_ID);
    const clientSecret = sessionStorage.getItem(SS_CLIENT_SECRET);

    sessionStorage.removeItem(SS_VERIFIER);
    sessionStorage.removeItem(SS_YT_IDS);
    sessionStorage.removeItem(SS_PLAYLIST_NAME);
    sessionStorage.removeItem(SS_CLIENT_ID);
    sessionStorage.removeItem(SS_CLIENT_SECRET);

    if (!verifier || !ytIdsRaw || !playlistName || !clientId || !clientSecret) {
      onErrorRef.current("YouTube OAuth: missing session data");
      return;
    }

    const ytIds: string[] = JSON.parse(ytIdsRaw);

    (async () => {
      try {
        setPhase({ kind: "exchanging-token" });
        const accessToken = await exchangeToken(
          code,
          verifier,
          clientId,
          clientSecret,
        );

        setPhase({ kind: "creating-playlist" });
        const playlist = await createYouTubePlaylist(accessToken, playlistName);

        for (let i = 0; i < ytIds.length; i++) {
          setPhase({ kind: "adding-tracks", current: i, total: ytIds.length });
          await addVideoToPlaylist(accessToken, playlist.id, ytIds[i]);
        }

        setPhase({ kind: "idle" });
        onSuccessRef.current(playlist.url, ytIds.length);
      } catch (e) {
        setPhase({ kind: "idle" });
        onErrorRef.current(String(e));
      }
    })();
  }, []);

  return phase;
}

// ─── Progress dialog shown after OAuth redirect ───────────────────────────────

export function OAuthProgressDialog({ phase }: { phase: OAuthPhase }) {
  const isOpen = phase.kind !== "idle";

  const label =
    phase.kind === "exchanging-token"
      ? "Connecting to Google…"
      : phase.kind === "creating-playlist"
        ? "Creating playlist…"
        : phase.kind === "adding-tracks"
          ? `Adding tracks… (${phase.current} / ${phase.total})`
          : "";

  const progressValue =
    phase.kind === "adding-tracks" && phase.total > 0
      ? Math.round((phase.current / phase.total) * 100)
      : undefined;

  return (
    <Dialog.Root open={isOpen}>
      <Dialog.Content maxWidth="400px">
        <Dialog.Title>Exporting to YouTube playlist</Dialog.Title>
        <Flex direction="column" gap="3" mt="2">
          <Flex align="center" gap="2">
            {phase.kind !== "adding-tracks" && <Spinner />}
            <Text size="2">{label}</Text>
          </Flex>
          <Progress value={progressValue} size="2" />
          {phase.kind === "adding-tracks" && (
            <Text size="1" color="gray">
              {phase.current} of {phase.total} tracks added
            </Text>
          )}
        </Flex>
      </Dialog.Content>
    </Dialog.Root>
  );
}

// ─── Dialog component ─────────────────────────────────────────────────────────

type YouTubePlaylistDialogProps = {
  open: boolean;
  ytIds: string[];
  onOpenChange: (open: boolean) => void;
};

export function YouTubePlaylistDialog({
  open,
  ytIds,
  onOpenChange,
}: YouTubePlaylistDialogProps) {
  const [clientId, setClientId] = useLocalStorage("yt_oauth_client_id", "");
  const [clientSecret, setClientSecret] = useLocalStorage(
    "yt_oauth_client_secret",
    "",
  );
  const [playlistName, setPlaylistName] = useState("stereo");

  const handleExport = async () => {
    if (!clientId || !clientSecret) return;

    const verifier = generateCodeVerifier();
    const challenge = await generateCodeChallenge(verifier);

    sessionStorage.setItem(SS_VERIFIER, verifier);
    sessionStorage.setItem(SS_YT_IDS, JSON.stringify(ytIds));
    sessionStorage.setItem(SS_PLAYLIST_NAME, playlistName);
    sessionStorage.setItem(SS_CLIENT_ID, clientId);
    sessionStorage.setItem(SS_CLIENT_SECRET, clientSecret);

    const authParams = new URLSearchParams({
      client_id: clientId,
      redirect_uri: window.location.origin,
      response_type: "code",
      scope: "https://www.googleapis.com/auth/youtube",
      code_challenge_method: "S256",
      code_challenge: challenge,
      state: OAUTH_STATE,
      access_type: "online",
    });

    window.location.href = `https://accounts.google.com/o/oauth2/v2/auth?${authParams}`;
  };

  const hasCredentials = !!clientId && !!clientSecret;

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Content maxWidth="440px">
        <Dialog.Title>Export to YouTube playlist</Dialog.Title>
        <Dialog.Description size="2" mb="4">
          Creates a private playlist on your YouTube account with {ytIds.length}{" "}
          track{ytIds.length !== 1 ? "s" : ""}. You'll be redirected to Google
          to authorize.
        </Dialog.Description>

        <Flex direction="column" gap="3">
          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Playlist name
            </Text>
            <TextField.Root
              value={playlistName}
              onChange={(e) => setPlaylistName(e.target.value)}
              placeholder="stereo"
            />
          </label>

          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Google OAuth client ID
            </Text>
            <TextField.Root
              value={clientId}
              onChange={(e) => setClientId(e.target.value)}
              placeholder="...apps.googleusercontent.com"
            />
          </label>

          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Google OAuth client secret
            </Text>
            <TextField.Root
              type="password"
              value={clientSecret}
              onChange={(e) => setClientSecret(e.target.value)}
              placeholder="GOCSPX-..."
            />
          </label>

          {!hasCredentials && (
            <Text size="1" color="gray">
              Create a <strong>Desktop app</strong> OAuth client in Google Cloud
              Console with the YouTube Data API v3 enabled. Register{" "}
              <Code>{window.location.origin}</Code> as an authorized redirect
              URI.
            </Text>
          )}
        </Flex>

        <Flex gap="3" mt="4" justify="end">
          <Dialog.Close>
            <Button variant="soft" color="gray">
              Cancel
            </Button>
          </Dialog.Close>
          <Button
            onClick={handleExport}
            disabled={!hasCredentials || !playlistName || ytIds.length === 0}
          >
            Connect &amp; export
          </Button>
        </Flex>
      </Dialog.Content>
    </Dialog.Root>
  );
}
