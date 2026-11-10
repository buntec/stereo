import { useState } from "react";
import { Text } from "@astryxdesign/core/Text";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { StatusDot } from "@astryxdesign/core/StatusDot";

import type { RequestReply, ServerMsg } from "./Types.tsx";

type YouTubeSessionDialogProps = {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  /** Browser the YouTube login is read from, or null when not connected. */
  browser: string | null;
  setBrowser: (browser: string | null) => void;
  requestReply: RequestReply;
};

function YouTubeSessionDialog({
  isOpen,
  onOpenChange,
  browser,
  setBrowser,
  requestReply,
}: YouTubeSessionDialogProps) {
  const [connecting, setConnecting] = useState(false);

  const onReply = (r: ServerMsg | { type: string }) => {
    if (r.type === "youtube-session" && "browser" in r) setBrowser(r.browser);
  };

  const connect = () => {
    setConnecting(true);
    requestReply(
      // The login is read from the browser this tab runs in.
      { type: "connect-youtube-session", user_agent: navigator.userAgent },
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
              browser ? (
                <HStack gap={1} vAlign="center">
                  <StatusDot variant="success" label="Connected" />
                  <Text type="supporting" color="secondary">
                    Connected via {browser}
                  </Text>
                </HStack>
              ) : undefined
            }
          />
        }
        content={
          <LayoutContent>
            <VStack gap={3}>
              <Text as="div" type="body">
                Stereo creates playlists on your YouTube account using the
                YouTube login of this browser. Make sure you&rsquo;re logged in
                to{" "}
                <Link href="https://www.youtube.com" isExternalLink>
                  youtube.com
                </Link>{" "}
                here with the account you want playlists on, then connect.
              </Text>
              <Text as="div" type="supporting" color="secondary">
                Stereo reads your YouTube cookies from this browser each time it
                exports a playlist and never keeps a copy, so logging out of
                YouTube here disconnects it too. It uses YouTube&rsquo;s own,
                undocumented web API, which has no quota but may change without
                notice.
              </Text>
            </VStack>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              {browser && (
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
              <Button
                label={browser ? "Reconnect" : "Connect"}
                variant="primary"
                onClick={connect}
                isLoading={connecting}
                isDisabled={connecting}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

export default YouTubeSessionDialog;
