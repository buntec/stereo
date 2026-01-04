import { useMemo } from "react";
import "./App.css";
import {
  ExclamationTriangleIcon,
  InfoCircledIcon,
} from "@radix-ui/react-icons";
import { Toast } from "radix-ui";

import { Callout } from "@radix-ui/themes";

type NotificationProps = {
  open: boolean;
  setOpen: (open: boolean) => void;
  message?: string;
  kind?: "info" | "warn" | "warning" | "error";
};

function Notification({ open, setOpen, message, kind }: NotificationProps) {
  const color = useMemo(() => {
    switch (kind) {
      case "info":
        return "blue";
      case "warn":
      case "warning":
        return "orange";
      case "error":
        return "red";
    }
  }, [kind]);

  const icon = useMemo(() => {
    switch (kind) {
      case "info":
        return <InfoCircledIcon />;
      case "warn":
      case "warning":
      case "error":
        return <ExclamationTriangleIcon />;
    }
  }, [kind]);

  return (
    <Toast.Root className="ToastRoot" open={open} onOpenChange={setOpen}>
      <Toast.Description asChild>
        <Callout.Root variant="surface" color={color}>
          <Callout.Icon>{icon}</Callout.Icon>
          <Callout.Text>{message}</Callout.Text>
        </Callout.Root>
      </Toast.Description>
    </Toast.Root>
  );
}

export default Notification;
