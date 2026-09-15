import { useEffect, useState } from "react";
import { Text } from "@astryxdesign/core/Text";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Code } from "@astryxdesign/core/Code";
import { Badge } from "@astryxdesign/core/Badge";

import type { Action, RequestReply } from "./Types.tsx";

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
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  requestReply: RequestReply;
  connected: boolean;
  clientId: string | null;
};

// Mount fresh for each opening (via `key`) so the inputs start from the saved
// client ID.
function YouTubeSessionDialog({
  isOpen,
  onOpenChange,
  requestReply,
  connected,
  clientId,
}: YouTubeSessionDialogProps) {
  const [clientIdInput, setClientIdInput] = useState(clientId ?? "");
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

  const closeDialog = () => onOpenChange(false);

  return (
    <Dialog isOpen={isOpen} onOpenChange={onOpenChange} width={480}>
      <Layout
        header={
          <DialogHeader
            title="YouTube Account"
            onOpenChange={closeDialog}
            endContent={
              connected ? (
                <Badge variant="success" label="Connected" />
              ) : undefined
            }
          />
        }
        content={
          <LayoutContent>
            <VStack gap={3}>
              <Text as="div" type="body">
                Stereo needs a Google OAuth client to create playlists on your
                behalf. Create a <strong>Desktop app</strong> OAuth client in
                Google Cloud Console with the YouTube Data API v3 enabled, and
                register <Code>{window.location.origin}</Code> as an authorized
                redirect URI.
              </Text>

              <TextInput
                label="Google OAuth client ID"
                value={clientIdInput}
                onChange={setClientIdInput}
                placeholder="...apps.googleusercontent.com"
              />

              <TextInput
                label="Google OAuth client secret"
                type="password"
                value={clientSecretInput}
                onChange={setClientSecretInput}
                placeholder={
                  hasStoredCredentials
                    ? "Leave blank to keep the saved secret"
                    : "GOCSPX-..."
                }
              />
            </VStack>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              {connected && (
                <Button
                  label="Disconnect"
                  variant="destructive"
                  onClick={() => {
                    requestReply(
                      { type: "disconnect-youtube-session" },
                      () => {},
                    );
                    closeDialog();
                  }}
                />
              )}
              <Button
                label="Cancel"
                variant="secondary"
                onClick={closeDialog}
              />
              <Button
                label="Connect"
                variant="primary"
                isDisabled={!canConnect}
                onClick={() => {
                  handleConnect();
                  closeDialog();
                }}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

export default YouTubeSessionDialog;
