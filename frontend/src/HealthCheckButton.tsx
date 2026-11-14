import { useState } from "react";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { List, ListItem } from "@astryxdesign/core/List";
import { ProgressBar } from "@astryxdesign/core/ProgressBar";
import { Spinner } from "@astryxdesign/core/Spinner";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { StatusDot } from "@astryxdesign/core/StatusDot";
import { Text } from "@astryxdesign/core/Text";
import {
  CheckIcon,
  CopyIcon,
  Cross2Icon,
  DotsHorizontalIcon,
  ReloadIcon,
} from "@radix-ui/react-icons";
// Radix has no health/diagnostics glyph. Lucide is the icon set Astryx's Neutral theme draws its
// own icons with, so it matches the dialogs and menus around it.
import { Stethoscope } from "lucide-react";
import type { HealthCheckResult, RequestReply, ServerMsg } from "./Types.tsx";

type HealthReport = Extract<ServerMsg, { type: "health-check" }>;

const STATUS_DOT = {
  running: {
    variant: "neutral",
    label: "Running",
    icon: <DotsHorizontalIcon />,
  },
  passed: { variant: "success", label: "Passed", icon: <CheckIcon /> },
  failed: { variant: "error", label: "Failed", icon: <Cross2Icon /> },
} as const;

function reportText(report: HealthReport, version?: string) {
  return [
    `Stereo ${version ? `v${version} ` : ""}health check — ${report.checked_at}`,
    ...report.checks.map(
      (check) =>
        `${check.status.toUpperCase()} ${check.name} (${check.duration_ms} ms)\n  ${check.probe}\n  ${check.message}`,
    ),
  ].join("\n\n");
}

export default function HealthCheckButton({
  isConnected,
  requestReply,
  version,
}: {
  isConnected: boolean;
  requestReply: RequestReply;
  version?: string;
}) {
  const [isOpen, setIsOpen] = useState(false);
  const [isRunning, setIsRunning] = useState(false);
  const [report, setReport] = useState<HealthReport | null>(null);
  // While a check runs: every check's state so far, from the backend's progress replies.
  const [progress, setProgress] = useState<HealthCheckResult[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const run = () => {
    if (isRunning || !isConnected) return;
    setIsRunning(true);
    setReport(null);
    setProgress(null);
    setError(null);
    setCopied(false);
    requestReply(
      { type: "run-health-check" },
      (reply) => {
        if (reply.type === "health-check-progress" && "checks" in reply) {
          setProgress(reply.checks);
          return;
        }
        setIsRunning(false);
        setProgress(null);
        if (reply.type === "health-check" && "checks" in reply) {
          setReport(reply);
        } else if ("message" in reply) {
          setError(reply.message);
        } else {
          setError(
            reply.type === "timeout"
              ? "The health check did not finish. Try again."
              : "Connection lost during the health check. Reconnect and try again.",
          );
        }
      },
      // Every backend probe is bounded to 45 s; allow for message batching and transit. If
      // this gives up first, the next run replaces the backend's (server.rs).
      60000,
    );
  };

  const checks = report?.checks ?? progress;
  const failed =
    checks?.filter((check) => check.status === "failed").length ?? 0;
  const finished =
    checks?.filter((check) => check.status !== "running").length ?? 0;

  return (
    <>
      <IconButton
        label="Health check"
        tooltip="Health check"
        icon={<Stethoscope size="1em" aria-hidden />}
        variant="ghost"
        onClick={() => {
          setIsOpen(true);
          // Merely reopening the report does not send another round of test requests.
          if (!report) run();
        }}
      />
      <Dialog isOpen={isOpen} onOpenChange={setIsOpen} width={640}>
        <Layout
          header={
            <DialogHeader
              title="Health check"
              subtitle="Check searches, charts and similar songs."
              onOpenChange={setIsOpen}
            />
          }
          content={
            <LayoutContent>
              <VStack gap={3}>
                <Text type="supporting">
                  Checks all five metadata providers, YouTube, Shazam and the
                  charts, regardless of your search settings. Each check has a
                  45-second timeout. Results reflect access from this backend; a
                  failure can also mean a network problem, rate limit or expired
                  browser clearance.
                </Text>
                {!isConnected && (
                  <Banner
                    status="warning"
                    title="Connect to Stereo to run a health check."
                  />
                )}
                {error && <Banner status="error" title={error} role="status" />}
                {isRunning && !progress && (
                  <Spinner label="Checking search services…" />
                )}
                {isRunning && progress && (
                  <ProgressBar
                    label={
                      failed === 0
                        ? "Checking search services…"
                        : `Checking search services… ${failed} failed so far`
                    }
                    value={finished}
                    max={progress.length}
                    hasValueLabel
                    formatValueLabel={(value, max) => `${value} of ${max}`}
                    variant={failed === 0 ? "accent" : "error"}
                  />
                )}
                <VStack role="status" aria-live="polite">
                  {report && (
                    <Text weight="semibold">
                      {failed === 0
                        ? `All ${report.checks.length} checks passed.`
                        : `${failed} of ${report.checks.length} checks failed.`}
                    </Text>
                  )}
                </VStack>
                {checks && (
                  <>
                    {report && (
                      <Text type="supporting">
                        Checked {new Date(report.checked_at).toLocaleString()}
                      </Text>
                    )}
                    <List
                      header="Search providers and charts"
                      edge="flush"
                      hasDividers
                    >
                      {checks.map((check) => (
                        <ListItem
                          key={check.name}
                          align="first-line"
                          startContent={
                            // The icon gives the status a shape, so it doesn't rest on
                            // color and needs no text repeating the dot's label.
                            <StatusDot {...STATUS_DOT[check.status]} />
                          }
                          label={
                            <HStack gap={2} wrap="wrap" vAlign="center">
                              <Text weight="semibold">{check.name}</Text>
                              <Text type="supporting">
                                {check.status === "running"
                                  ? "Checking…"
                                  : `${(check.duration_ms / 1000).toFixed(1)} s`}
                              </Text>
                            </HStack>
                          }
                          description={
                            <VStack gap={1}>
                              <Text type="supporting">{check.probe}</Text>
                              {check.message && (
                                <Text type="supporting">{check.message}</Text>
                              )}
                            </VStack>
                          }
                        />
                      ))}
                    </List>
                  </>
                )}
              </VStack>
            </LayoutContent>
          }
          footer={
            <LayoutFooter>
              <HStack gap={2} hAlign="end" wrap="wrap">
                {report && navigator.clipboard?.writeText && (
                  <Button
                    label={copied ? "Copied" : "Copy report"}
                    icon={<CopyIcon />}
                    clickAction={async () => {
                      try {
                        await navigator.clipboard.writeText(
                          reportText(report, version),
                        );
                        setCopied(true);
                      } catch {
                        setError("Could not copy the report to the clipboard.");
                      }
                    }}
                  />
                )}
                <Button
                  label={
                    isRunning
                      ? "Checking…"
                      : report
                        ? "Run again"
                        : "Run checks"
                  }
                  variant="primary"
                  icon={<ReloadIcon />}
                  isLoading={isRunning}
                  isDisabled={!isConnected}
                  onClick={run}
                />
              </HStack>
            </LayoutFooter>
          }
        />
      </Dialog>
    </>
  );
}
