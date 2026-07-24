import { useMemo } from "react";
import "./App.css";
import { Toast } from "@astryxdesign/core/Toast";
import { VStack } from "@astryxdesign/core/Stack";

type NotificationProps = {
  open: boolean;
  setOpen: (open: boolean) => void;
  message?: string;
  kind?: "info" | "warn" | "warning" | "error";
};

function Notification({ open, setOpen, message, kind }: NotificationProps) {
  const type = useMemo<"info" | "error">(() => {
    switch (kind) {
      case "warn":
      case "warning":
      case "error":
        return "error";
      default:
        return "info";
    }
  }, [kind]);

  if (!open || !message) return null;

  return (
    <VStack className="ToastViewport">
      <Toast
        body={message}
        type={type}
        isAutoHide={type === "info"}
        autoHideDuration={5000}
        onDismiss={() => setOpen(false)}
      />
    </VStack>
  );
}

export default Notification;
