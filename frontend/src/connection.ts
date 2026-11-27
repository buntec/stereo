// Messages and pairing codes for the connection to the backend.

const UNREACHABLE = "Can't reach Stereo";
const LOST = "Lost the connection to Stereo";

/** The toast for a closed connection, which the page keeps retrying. Browsers give no reason for
 * a failed handshake, e.g. a backend that is down or unreachable, so most have none. */
export const connectionLostMessage = (
  wasConnected: boolean,
  reason: string,
) => {
  const what = wasConnected
    ? `${LOST}. Reconnecting…`
    : `${UNREACHABLE}. Check that it's running and that this device can reach it. Retrying…`;
  return reason ? `${what} (${reason})` : what;
};

/** Whether a toast is `connectionLostMessage`'s, of which one shows at a time until reconnected. */
export const isConnectionMessage = (message: string) =>
  message.startsWith(UNREACHABLE) || message.startsWith(LOST);

/** A pairing code as typed: Crockford's base32 (`PAIRING_ALPHABET` in tailscale.rs), case and
 * separators ignored, the letters it leaves out read as the digits they look like. */
export const normalizePairingCode = (typed: string) =>
  typed
    .toUpperCase()
    .replace(/[^0-9A-Z]/g, "")
    .replace(/O/g, "0")
    .replace(/[IL]/g, "1");

export const PAIRING_CODE_LENGTH = 10;

/** A pairing code shown for typing: in two groups of five. */
export const formatPairingCode = (code: string) =>
  code.length === PAIRING_CODE_LENGTH
    ? `${code.slice(0, 5)}-${code.slice(5)}`
    : code;
