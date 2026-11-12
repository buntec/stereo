import { useState } from "react";
import { Text } from "@astryxdesign/core/Text";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { RadioList, RadioListItem } from "@astryxdesign/core/RadioList";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { StatusDot } from "@astryxdesign/core/StatusDot";

import {
  youTubeConnection,
  type RequestReply,
  type ServerMsg,
  type YouTubeConnection,
  type YouTubeLogin,
} from "./Types.tsx";
import { inDesktopApp, openInBrowser } from "./Utils.tsx";

type YouTubeSessionDialogProps = {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  /** The connected YouTube login, or null when not connected. */
  connection: YouTubeConnection | null;
  setConnection: (connection: YouTubeConnection | null) => void;
  requestReply: RequestReply;
};

const describe = ({ browser, account }: YouTubeConnection) =>
  account ? `${account} in ${browser}` : browser;

function YouTubeSessionDialog({
  isOpen,
  onOpenChange,
  connection,
  setConnection,
  requestReply,
}: YouTubeSessionDialogProps) {
  const [connecting, setConnecting] = useState(false);
  // Set when several browser profiles are logged in, for the user to pick one.
  const [logins, setLogins] = useState<YouTubeLogin[] | null>(null);
  const [picked, setPicked] = useState("");

  const onReply = (r: ServerMsg | { type: string }) => {
    if (r.type === "youtube-session" && "browser" in r) {
      setConnection(youTubeConnection(r));
      setLogins(null);
    } else if (r.type === "youtube-logins" && "logins" in r) {
      setLogins(r.logins);
      setPicked(r.logins[0]?.id ?? "");
    }
  };

  const connect = (login?: string) => {
    setConnecting(true);
    requestReply(
      // The login is read from the browser this tab runs in.
      {
        type: "connect-youtube-session",
        user_agent: navigator.userAgent,
        login,
      },
      (r) => {
        setConnecting(false);
        onReply(r);
      },
      // Reading the browser's cookies may wait on an OS keychain prompt.
      120000,
    );
  };

  const closeDialog = () => onOpenChange(false);

  return (
    <Dialog isOpen={isOpen} onOpenChange={onOpenChange} width={480}>
      <Layout
        header={
          <DialogHeader
            title="YouTube"
            onOpenChange={closeDialog}
            endContent={
              connection ? (
                <HStack gap={1} vAlign="center">
                  <StatusDot variant="success" label="Connected" />
                  <Text type="supporting" color="secondary">
                    Connected via {connection.browser}
                  </Text>
                </HStack>
              ) : undefined
            }
          />
        }
        content={
          <LayoutContent>
            {logins ? (
              <RadioList
                label="Choose a YouTube login"
                value={picked}
                onChange={setPicked}
              >
                {logins.map((login) => (
                  <RadioListItem
                    key={login.id}
                    value={login.id}
                    label={login.account ?? login.browser}
                    description={login.account ? login.browser : undefined}
                  />
                ))}
              </RadioList>
            ) : (
              <VStack gap={3}>
                {connection && (
                  <Text as="div" type="body">
                    Playlists are created as {describe(connection)}.
                  </Text>
                )}
                <Text as="div" type="body">
                  In {inDesktopApp ? "your web browser" : "this browser"}, log
                  in to{" "}
                  <Link href="https://www.youtube.com" isExternalLink>
                    youtube.com
                  </Link>{" "}
                  with the account or channel for your playlists, then connect
                  {inDesktopApp &&
                    " from Stereo in that browser. This app shares the connection"}
                  .
                </Text>
                <Text as="div" type="supporting" color="secondary">
                  Cookies are read on each export, never stored.
                </Text>
              </VStack>
            )}
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              {connection && !logins && (
                <Button
                  label="Disconnect"
                  variant="destructive"
                  onClick={() => {
                    requestReply(
                      { type: "disconnect-youtube-session" },
                      onReply,
                    );
                    closeDialog();
                  }}
                />
              )}
              <Button label="Close" variant="secondary" onClick={closeDialog} />
              {inDesktopApp ? (
                // The login is looked for in the browser the page runs in,
                // which this app's webview isn't.
                <Button
                  label="Open Stereo in browser"
                  variant="primary"
                  onClick={openInBrowser}
                />
              ) : (
                <Button
                  label={connection && !logins ? "Reconnect" : "Connect"}
                  variant="primary"
                  onClick={() => connect(logins ? picked : undefined)}
                  isLoading={connecting}
                  isDisabled={connecting || (logins !== null && !picked)}
                />
              )}
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

export default YouTubeSessionDialog;
