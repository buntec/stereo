import { useEffect, useMemo, useState, type SVGProps } from "react";
import { encode } from "uqr";
import { Button } from "@astryxdesign/core/Button";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Popover } from "@astryxdesign/core/Popover";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { StatusDot } from "@astryxdesign/core/StatusDot";
import { Text } from "@astryxdesign/core/Text";
import { CheckIcon, CopyIcon } from "@radix-ui/react-icons";
import type { RequestReply, TailscaleStatus } from "./Types.tsx";
import { formatPairingCode } from "./connection.ts";

// Tailscale's nine-dot mark. Sized in em: WebKit collapses an unsized SVG flex item to 0x0.
function TailscaleGlyph(props: SVGProps<SVGSVGElement>) {
  const lit = new Set([3, 4, 5, 7]);
  return (
    <svg
      viewBox="0 0 24 24"
      width="1em"
      height="1em"
      fill="currentColor"
      {...props}
    >
      {Array.from({ length: 9 }, (_, i) => (
        <circle
          key={i}
          cx={4 + (i % 3) * 8}
          cy={4 + Math.floor(i / 3) * 8}
          r={3}
          fillOpacity={lit.has(i) ? 1 : 0.3}
        />
      ))}
    </svg>
  );
}

// Black modules on white whatever the theme (see `.tailscale-qr` in App.css).
function QrCode({ text }: { text: string }) {
  const { size, path } = useMemo(() => {
    const { data, size } = encode(text, { ecc: "M", border: 2 });
    let path = "";
    data.forEach((row, y) =>
      row.forEach((dark, x) => {
        if (dark) path += `M${x} ${y}h1v1h-1z`;
      }),
    );
    return { size, path };
  }, [text]);
  return (
    <svg
      className="tailscale-qr"
      viewBox={`0 0 ${size} ${size}`}
      shapeRendering="crispEdges"
      role="img"
      aria-label="QR code of a sign-in link"
    >
      <rect className="tailscale-qr-light" width={size} height={size} />
      <path className="tailscale-qr-dark" d={path} />
    </svg>
  );
}

/** A single-use sign-in link for the tailnet URL, renewed before it expires. */
function PairingQr({
  url,
  requestReply,
}: {
  url: string;
  requestReply: RequestReply;
}) {
  const [code, setCode] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  // Bumped for a new code before the current one is due for renewal: a scan uses it up, and
  // the popover can't tell, so one meant for typing would otherwise be refused.
  const [renewals, setRenewals] = useState(0);
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const issue = () =>
      requestReply({ type: "create-pairing-code" }, (r) => {
        if (disposed) return;
        if (r.type === "pairing-code" && "code" in r) {
          setCode(r.code);
          setFailed(false);
          timer = setTimeout(issue, r.expires_in * 750);
        } else {
          setFailed(true);
        }
      });
    issue();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [requestReply, renewals]);
  const renew = (
    <Button
      label="Get a new code"
      variant="secondary"
      size="sm"
      onClick={() => setRenewals((n) => n + 1)}
    />
  );
  if (failed)
    return (
      <VStack gap={1} hAlign="center">
        <Text type="supporting">Could not create a sign-in code.</Text>
        {renew}
      </VStack>
    );
  if (!code) return <svg className="tailscale-qr" aria-hidden />;
  return (
    <VStack gap={1} hAlign="center">
      <QrCode text={`${url}/#stereo-pair=${code}`} />
      <Text type="supporting">
        Scan on a device in your tailnet, or enter this code where Stereo asks
        for one, e.g. in a Home Screen web app:
      </Text>
      <Text type="code" weight="bold" aria-label="Sign-in code">
        {formatPairingCode(code)}
      </Text>
      <Text type="supporting">
        Each code works once, for two minutes. Get a new one after scanning this
        one.
      </Text>
      {renew}
    </VStack>
  );
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(t);
  }, [copied]);
  // Absent outside secure contexts, e.g. plain HTTP from another machine.
  if (!navigator.clipboard?.writeText) return null;
  return (
    <IconButton
      label="Copy address"
      tooltip={copied ? "Copied" : "Copy address"}
      variant="ghost"
      size="sm"
      icon={copied ? <CheckIcon /> : <CopyIcon />}
      onClick={() =>
        navigator.clipboard.writeText(text).then(() => setCopied(true))
      }
    />
  );
}

// The popover's content stays mounted while closed; codes are issued only while open.
function Details({
  status,
  open,
  requestReply,
}: {
  status: TailscaleStatus;
  open: boolean;
  requestReply: RequestReply;
}) {
  if (status.state === "starting")
    return <Text color="secondary">Setting up tailscale serve…</Text>;
  if (status.state === "error")
    return (
      <VStack gap={1}>
        <Text>{status.message}</Text>
        <Text type="supporting">Retrying…</Text>
      </VStack>
    );
  return (
    <VStack gap={2}>
      <Text type="label" weight="bold">
        Tailnet
      </Text>
      <HStack gap={1} vAlign="center">
        <Text type="code" wordBreak="break-all">
          {status.url}
        </Text>
        <CopyButton text={status.url} />
      </HStack>
      {open && <PairingQr url={status.url} requestReply={requestReply} />}
    </VStack>
  );
}

/** The tailnet address of a backend run with `--tailscale`, with a QR code to sign in a phone. */
function TailscaleButton({
  status,
  requestReply,
}: {
  status?: TailscaleStatus;
  requestReply: RequestReply;
}) {
  const [open, setOpen] = useState(false);
  if (!status) return null;
  const label =
    status.state === "serving"
      ? "Tailnet"
      : status.state === "error"
        ? "Tailnet: not serving"
        : "Tailnet: starting";
  return (
    <HStack gap={0.5} vAlign="center">
      <Popover
        label="Tailnet"
        placement="below"
        alignment="end"
        width={320}
        onOpenChange={setOpen}
        content={
          <Details status={status} open={open} requestReply={requestReply} />
        }
      >
        <IconButton
          label={label}
          tooltip={label}
          variant="ghost"
          icon={
            <TailscaleGlyph opacity={status.state === "serving" ? 1 : 0.5} />
          }
        />
      </Popover>
      {status.state === "error" && (
        <StatusDot variant="warning" label="Tailnet not serving" />
      )}
    </HStack>
  );
}

export default TailscaleButton;
