import { useEffect, useState } from "react";
import {
  Text,
  Button,
  Dialog,
  IconButton,
  Tooltip,
  TextField,
  Flex,
  Code,
  Badge,
} from "@radix-ui/themes";

import type { Action, RequestReply } from "./Types.tsx";
import YouTubeIcon from "./YouTubeIcon.tsx";

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

const SS_VERIFIER = "yt_oauth_verifier";
const SS_REDIRECT_URI = "yt_oauth_redirect_uri";
const OAUTH_STATE = "stereo-yt-login";

function startOAuth(clientId: string) {
  const verifier = generateCodeVerifier();
  generateCodeChallenge(verifier).then((challenge) => {
    sessionStorage.setItem(SS_VERIFIER, verifier);
    sessionStorage.setItem(SS_REDIRECT_URI, window.location.origin);

    const authParams = new URLSearchParams({
      client_id: clientId,
      redirect_uri: window.location.origin,
      response_type: "code",
      scope: "https://www.googleapis.com/auth/youtube",
      code_challenge_method: "S256",
      code_challenge: challenge,
      state: OAUTH_STATE,
      access_type: "offline",
      prompt: "consent",
    });

    window.location.href = `https://accounts.google.com/o/oauth2/v2/auth?${authParams}`;
  });
}

// ─── Hook: handle OAuth callback on app load ──────────────────────────────────

export function useYouTubeOAuthCallback(
  requestReply: RequestReply,
  dispatch: (action: Action) => void,
) {
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const code = params.get("code");
    const state = params.get("state");
    if (!code || state !== OAUTH_STATE) return;

    history.replaceState({}, "", window.location.pathname);

    const verifier = sessionStorage.getItem(SS_VERIFIER);
    const redirectUri = sessionStorage.getItem(SS_REDIRECT_URI);
    sessionStorage.removeItem(SS_VERIFIER);
    sessionStorage.removeItem(SS_REDIRECT_URI);

    if (!verifier || !redirectUri) {
      dispatch({
        type: "notification",
        message: "YouTube sign-in: missing session data, please try again",
        kind: "error",
      });
      return;
    }

    requestReply(
      {
        type: "exchange-youtube-code",
        code,
        verifier,
        redirect_uri: redirectUri,
      },
      (r) => {
        if ("connected" in r && r.connected) {
          dispatch({
            type: "notification",
            message: "Connected to YouTube",
            kind: "info",
          });
        }
      },
      15000,
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
}

// ─── Dialog component ─────────────────────────────────────────────────────────

type YouTubeSessionDialogProps = {
  requestReply: RequestReply;
  connected: boolean;
  clientId: string | null;
};

function YouTubeSessionDialog({
  requestReply,
  connected,
  clientId,
}: YouTubeSessionDialogProps) {
  const [clientIdInput, setClientIdInput] = useState("");
  const [clientSecretInput, setClientSecretInput] = useState("");

  const hasStoredCredentials = !!clientId;

  const handleConnect = () => {
    const id = clientIdInput.trim();
    const secret = clientSecretInput.trim();
    if (secret) {
      requestReply(
        {
          type: "set-youtube-credentials",
          client_id: id,
          client_secret: secret,
        },
        (r) => {
          if ("client_id" in r && r.client_id) startOAuth(r.client_id);
        },
      );
    } else if (hasStoredCredentials) {
      startOAuth(id);
    }
  };

  const canConnect =
    !!clientIdInput.trim() &&
    (!!clientSecretInput.trim() || hasStoredCredentials);

  return (
    <Dialog.Root
      onOpenChange={(open) => {
        if (open) {
          setClientIdInput(clientId ?? "");
          setClientSecretInput("");
        }
      }}
    >
      <Tooltip content="Connect a YouTube account (for creating playlists)">
        <Dialog.Trigger>
          <IconButton variant={connected ? "solid" : "soft"} color="red">
            <YouTubeIcon />
          </IconButton>
        </Dialog.Trigger>
      </Tooltip>

      <Dialog.Content maxWidth="480px">
        <Dialog.Title>
          YouTube Account{" "}
          {connected && (
            <Badge color="green" ml="2">
              Connected
            </Badge>
          )}
        </Dialog.Title>
        <Dialog.Description size="2" mb="4">
          Stereo needs a Google OAuth client to create playlists on your behalf.
          Create a <strong>Desktop app</strong> OAuth client in Google Cloud
          Console with the YouTube Data API v3 enabled, and register{" "}
          <Code>{window.location.origin}</Code> as an authorized redirect URI.
        </Dialog.Description>

        <Flex direction="column" gap="3">
          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Google OAuth client ID
            </Text>
            <TextField.Root
              value={clientIdInput}
              onChange={(e) => setClientIdInput(e.target.value)}
              placeholder="...apps.googleusercontent.com"
            />
          </label>

          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Google OAuth client secret
            </Text>
            <TextField.Root
              type="password"
              value={clientSecretInput}
              onChange={(e) => setClientSecretInput(e.target.value)}
              placeholder={
                hasStoredCredentials
                  ? "Leave blank to keep the saved secret"
                  : "GOCSPX-..."
              }
            />
          </label>
        </Flex>

        <Flex gap="3" mt="4" justify="end">
          {connected && (
            <Dialog.Close>
              <Button
                variant="soft"
                color="red"
                onClick={() =>
                  requestReply({ type: "disconnect-youtube-session" }, () => {})
                }
              >
                Disconnect
              </Button>
            </Dialog.Close>
          )}
          <Dialog.Close>
            <Button variant="soft" color="gray">
              Cancel
            </Button>
          </Dialog.Close>
          <Button color="red" disabled={!canConnect} onClick={handleConnect}>
            Connect
          </Button>
        </Flex>
      </Dialog.Content>
    </Dialog.Root>
  );
}

export default YouTubeSessionDialog;
