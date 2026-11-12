import { useState } from "react";
import { Text } from "@astryxdesign/core/Text";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { StatusDot } from "@astryxdesign/core/StatusDot";

import type { RequestReply, ServerMsg } from "./Types.tsx";

type BeatportSessionDialogProps = {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  /** Browser the connected clearance came from, or null when not connected. */
  browser: string | null;
  setBrowser: (browser: string | null) => void;
  requestReply: RequestReply;
};

function BeatportSessionDialog({
  isOpen,
  onOpenChange,
  browser,
  setBrowser,
  requestReply,
}: BeatportSessionDialogProps) {
  const [connecting, setConnecting] = useState(false);

  const onReply = (r: ServerMsg | { type: string }) => {
    if (r.type === "beatport-session" && "browser" in r) setBrowser(r.browser);
  };

  const connect = () => {
    setConnecting(true);
    requestReply(
      // Cloudflare only honors the clearance together with the User-Agent of
      // the browser that earned it, which is the one this tab runs in.
      { type: "connect-beatport-session", user_agent: navigator.userAgent },
      (r) => {
        setConnecting(false);
        onReply(r);
      },
      // Reading the browser's cookies may wait on an OS keychain prompt, and
      // each clearance found is then tried with a Beatport search.
      120000,
    );
  };

  const closeDialog = () => onOpenChange(false);

  return (
    <Dialog isOpen={isOpen} onOpenChange={onOpenChange} width={480}>
      <Layout
        header={
          <DialogHeader
            title="Beatport"
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
                In this browser,{" "}
                <Link
                  href="https://www.beatport.com/search?q=stereo"
                  isExternalLink
                >
                  search beatport.com
                </Link>{" "}
                and pass its bot check, then connect.
              </Text>
              <Text as="div" type="supporting" color="secondary">
                Only bot-check cookies are read. Reconnect when searches fail
                again.
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
                      { type: "disconnect-beatport-session" },
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

export default BeatportSessionDialog;
