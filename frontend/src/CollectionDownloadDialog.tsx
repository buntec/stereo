import { useEffect } from "react";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { Button } from "@astryxdesign/core/Button";
import { HStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { Code } from "@astryxdesign/core/Code";

export type ReadyDownload = { url: string; fileName: string };

// The server forgets a prepared download after two minutes.
const LIFETIME_MS = 110_000;

/** In the desktop app, offers a prepared collection download as a link to click. The webview
 * has no downloads, so the link opens in the browser, which saves the file. A window opened
 * from script once the snapshot is ready, after the menu click, could be blocked as a popup
 * (WebKitGTK's default); one opened by clicking a link can't. */
function CollectionDownloadDialog({
  download,
  onClose,
}: {
  download: ReadyDownload | null;
  onClose: () => void;
}) {
  useEffect(() => {
    if (!download) return;
    const timer = setTimeout(onClose, LIFETIME_MS);
    return () => clearTimeout(timer);
  }, [download, onClose]);

  return (
    <Dialog
      isOpen={download !== null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      width={380}
    >
      <Layout
        header={
          <DialogHeader title="Download Collection" onOpenChange={onClose} />
        }
        content={
          <LayoutContent>
            <Text as="div" type="body">
              <Code>{download?.fileName}</Code> is ready. It downloads in your
              web browser.
            </Text>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              <Button label="Cancel" variant="secondary" onClick={onClose} />
              <Button
                label="Download"
                variant="primary"
                href={download?.url}
                target="_blank"
                rel="noopener noreferrer"
                // After the click follows the link, which a link already removed can't.
                onClick={() => setTimeout(onClose)}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

export default CollectionDownloadDialog;
