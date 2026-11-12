// The WebSocket token (`--ws-token`) this page connects with.
//
// Kept in localStorage, so a phone stays signed in when the app is reopened, e.g. from the home
// screen. Older versions kept it in sessionStorage. Forgotten once the backend rejects it, e.g.
// after a restart with another --ws-token (or none, which accepts it).

const WS_TOKEN_KEY = "stereo-ws-token";

const storedWsToken = () => {
  try {
    return (
      window.localStorage.getItem(WS_TOKEN_KEY) ??
      window.sessionStorage.getItem(WS_TOKEN_KEY)
    );
  } catch {
    return null;
  }
};

let wsToken = storedWsToken();

export const currentWsToken = () => wsToken;

export const rememberWsToken = (token: string) => {
  wsToken = token;
  try {
    window.localStorage.setItem(WS_TOKEN_KEY, token);
  } catch {
    // Still used for this page's reconnects.
  }
};

export const forgetWsToken = () => {
  wsToken = null;
  try {
    window.localStorage.removeItem(WS_TOKEN_KEY);
    window.sessionStorage.removeItem(WS_TOKEN_KEY);
  } catch {
    // Not stored, then.
  }
};
