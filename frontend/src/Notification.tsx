import { useEffect, useRef } from "react";
import { useToast, type ToastDismissFn } from "@astryxdesign/core/Toast";
import type { AppNotification } from "./Types";

type NotificationProps = {
  notifications: AppNotification[];
  onConsumed: (ids: number[]) => void;
  isConnected: boolean;
};

export default function Notification({
  notifications,
  onConsumed,
  isConnected,
}: NotificationProps) {
  const toast = useToast();
  const shown = useRef(0);
  const connectionToast = useRef<ToastDismissFn | undefined>(undefined);

  useEffect(() => {
    for (const notification of notifications) {
      // React StrictMode replays effects; each queued event is shown only once.
      if (notification.id <= shown.current) continue;
      shown.current = notification.id;
      const isConnection = notification.message.startsWith("WebSocket");
      const dismiss = toast({
        body: notification.message,
        type:
          notification.kind && notification.kind !== "info" ? "error" : "info",
        uniqueID: isConnection
          ? "connection"
          : `${notification.kind}:${notification.message}`,
        collisionBehavior: "overwrite",
      });
      if (isConnection) connectionToast.current = dismiss;
    }
    if (notifications.length) onConsumed(notifications.map((item) => item.id));
  }, [notifications, onConsumed, toast]);

  useEffect(() => {
    if (isConnected) {
      connectionToast.current?.();
      connectionToast.current = undefined;
    }
  }, [isConnected]);

  return null;
}
