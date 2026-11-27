import { useState } from "react";
import { Button } from "@astryxdesign/core/Button";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { HStack } from "@astryxdesign/core/Stack";
import { TextInput } from "@astryxdesign/core/TextInput";
import { normalizePairingCode, PAIRING_CODE_LENGTH } from "./connection.ts";

/**
 * Asks for the code the Tailscale button shows beside its QR code, for a page the tailnet
 * listener turned away for having no token (`SIGN_IN_REQUIRED`). Scanning the QR code instead
 * would sign in Safari, not a Home Screen web app on iOS, which has its own storage.
 */
function PairingSignIn({
  error,
  isPending,
  onSubmit,
}: {
  /** Why the last code or saved sign-in didn't work. */
  error?: string;
  /** Whether a submitted code is being tried. */
  isPending: boolean;
  onSubmit: (code: string) => void;
}) {
  const [typed, setTyped] = useState("");
  const code = normalizePairingCode(typed);
  const isComplete = code.length === PAIRING_CODE_LENGTH;
  const submit = () => {
    if (isComplete && !isPending) onSubmit(code);
  };
  return (
    <HStack height="100%" hAlign="center" vAlign="center">
      <EmptyState
        title="Sign in to Stereo"
        description="This device isn't signed in yet. On a computer running Stereo, open the Tailscale menu in the top bar and enter the code shown under its QR code."
        actions={
          <HStack gap={2} vAlign="end" wrap="wrap" hAlign="center">
            <TextInput
              label="Sign-in code"
              placeholder="XXXXX-XXXXX"
              value={typed}
              onChange={setTyped}
              hasAutoFocus
              autoComplete="one-time-code"
              autoCapitalize="characters"
              hasAutoCorrect={false}
              hasSpellCheck={false}
              enterKeyHint="go"
              statusVariant="detached"
              status={error ? { type: "error", message: error } : undefined}
              onEnter={submit}
            />
            <Button
              label={isPending ? "Signing in…" : "Sign in"}
              variant="primary"
              isDisabled={!isComplete || isPending}
              onClick={submit}
            />
          </HStack>
        }
      />
    </HStack>
  );
}

export default PairingSignIn;
