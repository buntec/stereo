import {
  test,
  expect,
  type Locator,
  type Page,
  type Route,
  type WebSocketRoute,
} from "@playwright/test";
import { readFileSync } from "node:fs";
import { collectionFixture } from "./fixture-server.ts";

const exampleTracks = Array.from({ length: 12 }, (_, index) => ({
  title: `Example track ${index + 1}`,
  artists: ["Example artist"],
  yt_id: `audit${index}`,
  label: "Example label",
  release_date: "2026-11-01",
  bpm: 130,
  rating: 4,
}));
const errors = new WeakMap<Page, string[]>();
test.beforeEach(({ page }) => {
  errors.set(page, []);
  page.on("pageerror", (error) => errors.get(page)?.push(error.message));
});
test.afterEach(({ page }) => expect(errors.get(page)).toEqual([]));

async function openApp(
  page: Page,
  options: {
    empty?: boolean;
    noCollection?: boolean;
    holdRows?: boolean;
    /**
     * Downloaded audio, which the backend never sends (the default), serves as
     * `audioSeconds` of silence, fails to download, or doesn't offer at all.
     */
    audio?: "hold" | "ok" | "fail" | "off";
    audioSeconds?: number;
    /** What every search finds. */
    searchResults?: Record<string, unknown>[];
    /** Collection tracks like a search result, by the result's YouTube id. */
    similarTracks?: Record<string, Record<string, unknown>[]>;
    /** Leaves `validate-track` unanswered, for the test to answer itself. */
    holdValidation?: boolean;
    /** Shows the disco ball, hidden by default (in stored settings, unless there are some already). */
    discoBall?: boolean;
    /** How many tracks the collection has, more than a grid shows at once if need be. */
    trackCount?: number;
    /** Sends the repo's and collection's paths as a Windows backend does. */
    windowsPaths?: boolean;
    /** Further collections the repo has, by path, after the open one. */
    extraCollections?: string[];
  } = {},
) {
  if (options.discoBall)
    await page.addInitScript(() => {
      if (localStorage.getItem("stereo_app_settings")) return;
      localStorage.setItem(
        "stereo_app_settings",
        JSON.stringify({
          appearance: "system",
          repoPath: "",
          collectionPath: "",
          backgroundVideo: true,
          shufflePlay: false,
          discoBall: { isShown: true },
        }),
      );
    });
  let socket: WebSocketRoute;
  const tracks = options.empty
    ? []
    : options.trackCount === undefined
      ? structuredClone(exampleTracks)
      : Array.from({ length: options.trackCount }, (_, index) => ({
          ...exampleTracks[0],
          title: `Example track ${index + 1}`,
          yt_id: `audit${index}`,
        }));
  const repoPath = options.windowsPaths
    ? "C:\\Users\\me\\.local\\share\\stereo\\gitrepos\\audit"
    : "/audit";
  const collection = {
    path: `${repoPath}${options.windowsPaths ? "\\" : "/"}collection.db`,
    size: tracks.length,
  };
  const messages: Record<string, unknown>[] = [];
  /** Ids the collection answers `get-collection-track` as not containing. */
  const absent = new Set<string>();
  await page.route("https://www.youtube.com/**", (route) => route.abort());
  await page.route("https://i.ytimg.com/**", (route) => route.abort());
  const audioRequests: string[] = [];
  /** Requests answered with `hold`, which a test may still answer itself. */
  const heldAudio: Route[] = [];
  /** Replies to the queue's `get-rows` (the whole view), while held. */
  let holdQueue = false;
  const heldQueue: (() => void)[] = [];
  let audio = options.audio ?? "hold";
  if (audio !== "off")
    await page.route("**/audio/**", (route) => {
      const url = new URL(route.request().url());
      audioRequests.push(
        `${route.request().method()} ${url.pathname}?${url.searchParams}`,
      );
      if (audio === "hold") return void heldAudio.push(route);
      if (audio === "fail")
        return route.fulfill({ status: 502, body: "yt-dlp failed" });
      // Range requests as the backend serves them; Chromium only seeks with those.
      const wav = silentWav(options.audioSeconds ?? 1);
      const range = /bytes=(\d+)-(\d*)/.exec(
        route.request().headers()["range"] ?? "",
      );
      if (!range)
        return route.fulfill({
          status: 200,
          contentType: "audio/wav",
          headers: { "accept-ranges": "bytes" },
          body: wav,
        });
      const start = Number(range[1]);
      const end = range[2] ? Number(range[2]) : wav.length - 1;
      return route.fulfill({
        status: 206,
        contentType: "audio/wav",
        headers: {
          "accept-ranges": "bytes",
          "content-range": `bytes ${start}-${end}/${wav.length}`,
        },
        body: wav.subarray(start, end + 1),
      });
    });
  await page.routeWebSocket("**/ws", (ws) => {
    socket = ws;
    const send = (message: unknown) => ws.send(JSON.stringify(message));
    ws.onMessage((raw) => {
      const message = JSON.parse(raw.toString());
      messages.push(message);
      switch (message.type) {
        case "get-rows": {
          const reply = () =>
            send({
              type: "rows",
              id: message.id,
              rows:
                message.filterModel && Object.keys(message.filterModel).length
                  ? []
                  : tracks.slice(
                      message.startRow ?? 0,
                      message.endRow ?? tracks.length,
                    ),
              last_row:
                message.filterModel && Object.keys(message.filterModel).length
                  ? 0
                  : tracks.length,
            });
          if (options.holdRows) break;
          if (holdQueue && message.endRow === 10000) heldQueue.push(reply);
          else reply();
          break;
        }
        case "set-repo":
          send({
            type: "collections",
            collections: options.noCollection
              ? []
              : [
                  collection,
                  ...(options.extraCollections ?? []).map((path) => ({
                    path,
                    size: 0,
                  })),
                ],
          });
          break;
        case "set-collection":
          send({ type: "collection-info", id: message.id, collection });
          break;
        case "get-track-info": {
          const track = tracks.find((track) => track.yt_id === message.yt_id);
          if (track) send({ type: "track-info", track });
          break;
        }
        case "get-spotify-session":
          send({ type: "spotify-session", id: message.id, connected: true });
          break;
        case "get-youtube-session":
          send({
            type: "youtube-session",
            id: message.id,
            browser: null,
            account: null,
          });
          break;
        case "connect-youtube-session":
          send(
            message.login
              ? {
                  type: "youtube-session",
                  id: message.id,
                  browser: "Firefox (work)",
                  account: "Work Channel (@work)",
                }
              : {
                  type: "youtube-logins",
                  id: message.id,
                  logins: [
                    {
                      id: "/p/personal/cookies.sqlite",
                      browser: "Firefox (personal)",
                      account: "Me (@me)",
                    },
                    {
                      id: "/p/work/cookies.sqlite",
                      browser: "Firefox (work)",
                      account: "Work Channel (@work)",
                    },
                  ],
                },
          );
          break;
        case "search":
          send([
            {
              type: "search-results",
              query_id: message.query_id,
              tracks: options.searchResults ?? [],
            },
            { type: "search-complete", query_id: message.query_id },
          ]);
          break;
        case "search-youtube-videos": {
          // A pasted link gives just that video; searches find nothing.
          const linked = /[?&]v=([\w-]{11})/.exec(message.query)?.[1];
          send({
            type: "youtube-videos",
            id: message.id,
            videos: linked
              ? [
                  {
                    id: linked,
                    title: `Video ${linked}`,
                    channel: "Example channel",
                    duration_secs: 300,
                  },
                ]
              : [],
          });
          break;
        }
        case "validate-track":
          if (options.holdValidation) break;
          send({
            type: "validate-track-reply",
            id: message.id,
            is_valid: !!message.track.title.trim(),
          });
          break;
        case "update-rating": {
          const track = tracks.find((track) => track.yt_id === message.yt_id);
          if (track) track.rating = message.rating;
          break;
        }
        case "update-bpm": {
          const track = tracks.find((track) => track.yt_id === message.yt_id);
          if (track) {
            track.bpm = message.bpm;
            send({ type: "track-update", track });
          }
          break;
        }
        case "get-collection-track":
          send({
            type: "collection-track",
            id: message.id,
            track:
              (message.collection === collection.path &&
                !absent.has(message.yt_id) &&
                tracks.find((track) => track.yt_id === message.yt_id)) ||
              null,
          });
          break;
        case "find-similar-tracks":
          send({
            type: "similar-tracks",
            id: message.id,
            similar: message.tracks.map(
              (t: { yt_id: string }) => options.similarTracks?.[t.yt_id] ?? [],
            ),
          });
          break;
        case "collection-contains-ids":
          send({
            type: "collection-contains-ids-response",
            id: message.id,
            contained: message.yt_ids.filter((id: string) =>
              tracks.some((track) => track.yt_id === id),
            ),
          });
          break;
      }
    });
    setTimeout(
      () =>
        send([
          {
            type: "backend-info",
            version: "test",
            home_dir: "/audit",
            audio_key: audio === "off" ? null : "test-key",
          },
          { type: "repos", repos: [{ path: repoPath, remote: null }] },
        ]),
      20,
    );
  });
  await page.goto("/");
  await expect(
    page.locator(".collection-controls").getByRole("button").first(),
  ).toBeVisible();
  await expect(
    page.getByText("Connecting to Stereo…", { exact: true }),
  ).toHaveCount(0);
  return {
    messages,
    tracks,
    collection,
    absent,
    audioRequests,
    heldAudio,
    /** Holds the replies that set the queue to the collection's view, or sends them. */
    holdQueue: (hold: boolean) => {
      holdQueue = hold;
      if (!hold) heldQueue.splice(0).forEach((reply) => reply());
    },
    /** Changes how further audio requests are answered (not "off"). */
    setAudio: (mode: "hold" | "ok" | "fail") => {
      audio = mode;
    },
    send: (message: unknown) => socket.send(JSON.stringify(message)),
    disconnect: () => socket.close(),
  };
}

/**
 * A notification's text in the toast stack. Astryx also announces it in a live
 * region, so it's on the page twice.
 */
function toastText(
  page: Page,
  text: string | RegExp,
  options?: { exact?: boolean },
) {
  return page
    .getByRole("region", { name: "Notifications" })
    .getByText(text, options);
}

/**
 * The keydown of an Enter that commits an input method editor's text, which
 * mustn't submit a dialog's form.
 */
const composingEnter = {
  key: "Enter",
  code: "Enter",
  isComposing: true,
  bubbles: true,
  cancelable: true,
};

/**
 * Plays the collection's first track, which loading the page only selects.
 */
async function startPlayback(page: Page) {
  await expect(page.locator('.ag-row[row-index="0"]')).toBeVisible();
  const button = page.locator(".play-pause-button");
  await expect(button).toHaveAccessibleName("Play");
  await button.click();
}

/** A mono 8 kHz WAV file of silence. */
function silentWav(seconds: number): Buffer {
  const rate = 8000;
  const dataSize = rate * seconds * 2;
  const wav = Buffer.alloc(44 + dataSize);
  wav.write("RIFF", 0);
  wav.writeUInt32LE(36 + dataSize, 4);
  wav.write("WAVEfmt ", 8);
  wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20);
  wav.writeUInt16LE(1, 22);
  wav.writeUInt32LE(rate, 24);
  wav.writeUInt32LE(rate * 2, 28);
  wav.writeUInt16LE(2, 32);
  wav.writeUInt16LE(16, 34);
  wav.write("data", 36);
  wav.writeUInt32LE(dataSize, 40);
  return wav;
}

test("focused controls retain Space; global shortcuts work on the page", async ({
  page,
}) => {
  await openApp(page);
  const shuffle = page.getByRole("button", {
    name: "Toggle shuffle play",
    exact: true,
  });
  await shuffle.focus();
  await page.keyboard.press("Space");
  await expect(shuffle).toHaveAttribute("aria-pressed", "true");
  await page.locator("body").click({ position: { x: 1, y: 1 } });
  await page.keyboard.press("s");
  await expect(shuffle).toHaveAttribute("aria-pressed", "false");
  await page.getByRole("button", { name: /^Spotify:/ }).click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Cancel", exact: true })
    .focus();
  await page.keyboard.press("Space");
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("rating changes on click and has an explicit clear action", async ({
  page,
}) => {
  const { messages } = await openApp(page);
  const row = page.locator('.ag-row[row-index="0"]');
  const group = row.getByRole("group", { name: "Rating for Example track 1" });
  await expect(
    group.getByRole("button", { name: "4 of 5 stars" }),
  ).toHaveAttribute("aria-pressed", "true");
  await group.getByRole("button", { name: "5 of 5 stars" }).click();
  await expect(
    group.getByRole("button", { name: "5 of 5 stars" }),
  ).toHaveAttribute("aria-pressed", "true");
  expect(
    messages.some(
      (message) => message.type === "update-rating" && message.rating === 5,
    ),
  ).toBe(true);
  await row.getByRole("button", { name: "Clear rating" }).click();
  await expect
    .poll(() =>
      messages.some(
        (message) =>
          message.type === "update-rating" && message.rating === null,
      ),
    )
    .toBe(true);
});

test("notifications stack and a later notification gets its full lifetime", async ({
  page,
}) => {
  const { send } = await openApp(page);
  send([
    { type: "notification", kind: "error", message: "First warning" },
    { type: "notification", kind: "error", message: "Second warning" },
  ]);
  await expect(toastText(page, "First warning")).toBeVisible();
  await expect(toastText(page, "Second warning")).toBeVisible();
  send({ type: "notification", kind: "info", message: "First information" });
  await expect(toastText(page, "First information")).toBeVisible();
  await page.waitForTimeout(4200);
  send({ type: "notification", kind: "info", message: "Later information" });
  await page.waitForTimeout(1200);
  await expect(toastText(page, "Later information")).toBeVisible();
});

for (const width of [390, 768, 1440]) {
  test(`header and playback stay usable at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 844 });
    await openApp(page);
    await expect(page.getByRole("main")).toHaveCount(1);
    const expectOnScreen = async (control: Locator) => {
      await expect(control).toBeInViewport();
      const box = await control.boundingBox();
      expect(box!.x).toBeGreaterThanOrEqual(0);
      expect(box!.x + box!.width).toBeLessThanOrEqual(width);
    };
    for (const control of [
      page.locator(".collection-controls").getByRole("button").first(),
      page.getByRole("button", { name: /^YouTube:/ }),
      page.getByRole("button", { name: /^Spotify:/ }),
      page.getByRole("button", { name: /^Beatport:/ }),
      page.getByRole("button", { name: "Health check", exact: true }),
      page.getByRole("button", { name: "Search", exact: true }),
      page.getByRole("button", { name: "Toggle shuffle play", exact: true }),
    ]) {
      await expectOnScreen(control);
    }
    const grid = await page.locator(".grid").boundingBox();
    expect(grid!.height).toBeGreaterThan(300);
    // Up to 48rem the appearance switch moves from the nav bar into Settings.
    const appearance = page.getByRole("radiogroup", { name: "Appearance" });
    if (width <= 768) {
      await expect(appearance).toHaveCount(0);
      await page.getByRole("button", { name: "Settings", exact: true }).click();
    }
    await expectOnScreen(appearance);
    const dark = appearance.getByRole("radio", { name: "Dark" });
    await dark.click();
    await expect(dark).toBeChecked();
  });
}

for (const width of [390, 1440]) {
  test(`health check keeps a diagnostic report and supports reruns at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 844 });
    const { messages, send } = await openApp(page, { noCollection: true });
    const requests = () =>
      messages.filter((message) => message.type === "run-health-check");
    const trigger = page.getByRole("button", {
      name: "Health check",
      exact: true,
    });
    await trigger.click();
    const dialog = page.getByRole("dialog", { name: "Health check" });
    await expect(dialog).toBeVisible();
    await expect(
      dialog.getByRole("progressbar", { name: "Checking search services…" }),
    ).toBeVisible();
    await expect.poll(() => requests().length).toBe(1);
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    await expect(trigger).toBeFocused();
    await trigger.click();
    expect(requests()).toHaveLength(1);
    const checks = [
      "Beatport",
      "SoundCloud",
      "Volumo",
      "Traxsource",
      "Spotify",
      "YouTube",
      "Shazam",
      "Beatport charts",
      "Traxsource charts",
    ].map((name, index) => ({
      name,
      probe: "Search for “Solomun”",
      status: index === 0 ? "failed" : "passed",
      message:
        index === 0
          ? "request blocked by Cloudflare's bot check (HTTP 403). Reconnect Beatport to refresh the browser clearance."
          : "3 usable results; e.g. Solomun – Example track",
      duration_ms: 1234,
    }));
    send({
      type: "health-check",
      id: requests()[0].id,
      checked_at: "2026-11-13T12:00:00Z",
      checks,
    });
    await expect(dialog.getByText("1 of 9 checks failed.")).toBeVisible();
    await expect(dialog.getByText(checks[0].message)).toBeVisible();
    await expect(dialog.getByRole("listitem")).toHaveCount(9);
    const box = await dialog.boundingBox();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(width);
    const copy = dialog.getByRole("button", {
      name: "Copy report",
      exact: true,
    });
    await copy.scrollIntoViewIfNeeded();
    await page.evaluate(() => {
      Object.defineProperty(navigator.clipboard, "writeText", {
        configurable: true,
        value: async (text: string) => {
          document.documentElement.dataset.copiedReport = text;
        },
      });
    });
    await copy.click();
    await expect(
      dialog.getByRole("button", { name: "Copied", exact: true }),
    ).toBeVisible();
    expect(
      await page.locator("html").getAttribute("data-copied-report"),
    ).toContain("FAILED Beatport (1234 ms)");
    await dialog.getByRole("button", { name: "Close", exact: true }).click();
    await trigger.click();
    await expect(dialog.getByText("1 of 9 checks failed.")).toBeVisible();
    expect(requests()).toHaveLength(1);
    await dialog
      .getByRole("button", { name: "Run again", exact: true })
      .click();
    await expect.poll(() => requests().length).toBe(2);
    send({
      type: "health-check",
      id: requests()[1].id,
      checked_at: "2026-11-13T12:01:00Z",
      checks: checks.map((check) => ({ ...check, status: "passed" })),
    });
    await expect(dialog.getByText("All 9 checks passed.")).toBeVisible();
  });
}

test("health check recovers from a lost connection", async ({ page }) => {
  const { messages, send, disconnect } = await openApp(page);
  await page.getByRole("button", { name: "Health check", exact: true }).click();
  await expect
    .poll(() => messages.some((message) => message.type === "run-health-check"))
    .toBe(true);
  disconnect();
  const dialog = page.getByRole("dialog", { name: "Health check" });
  await expect(
    dialog.getByText(
      "Connection lost during the health check. Reconnect and try again.",
    ),
  ).toBeVisible();
  await expect(
    dialog.getByRole("button", { name: "Run checks", exact: true }),
  ).toBeEnabled();
  await dialog.getByRole("button", { name: "Run checks", exact: true }).click();
  await expect
    .poll(
      () =>
        messages.filter((message) => message.type === "run-health-check")
          .length,
    )
    .toBe(2);
  const request = messages.filter(
    (message) => message.type === "run-health-check",
  )[1];
  send({
    type: "health-check",
    id: request.id,
    checked_at: "2026-11-13T12:00:00Z",
    checks: [
      {
        name: "YouTube",
        probe: "Search for “Daft Punk One More Time”",
        status: "passed",
        message: "3 usable results; e.g. Daft Punk - One More Time",
        duration_ms: 800,
      },
    ],
  });
  await expect(dialog.getByText("All 1 checks passed.")).toBeVisible();
  await expect(
    dialog.getByText(
      "Connection lost during the health check. Reconnect and try again.",
    ),
  ).toHaveCount(0);
});

test("column customizations survive crossing the compact breakpoint", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 844 });
  await openApp(page);
  const header = (colId: string) =>
    page.locator(`.ag-header-cell[col-id="${colId}"]`);
  const resize = async (colId: string, by: number) => {
    const handle = header(colId).locator(".ag-header-cell-resize");
    const box = (await handle.boundingBox())!;
    const y = box.y + box.height / 2;
    await page.mouse.move(box.x + box.width / 2, y);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + by, y, { steps: 5 });
    await page.mouse.up();
  };
  const widthOf = async (colId: string) =>
    Math.round((await header(colId).boundingBox())!.width);

  // The grid state the app saves (and restores on reload): off-screen columns aren't
  // rendered, so their widths and visibility are read from there.
  const saved = async () => {
    const state = await page.evaluate(() =>
      JSON.parse(localStorage.getItem("stereo_app_grid_state_v2") ?? "{}"),
    );
    const sizing = (colId: string) =>
      state.columnSizing?.columnSizingModel?.find(
        (s: { colId: string }) => s.colId === colId,
      );
    return {
      title: sizing("title"),
      plays: sizing("play_count"),
      hidden: state.columnVisibility?.hiddenColIds ?? [],
    };
  };

  // Plays first: a wider Title pushes it off-screen.
  await resize("play_count", 70);
  await resize("title", 100);
  await page.getByRole("button", { name: "View", exact: true }).click();
  await page.getByRole("menuitemcheckbox", { name: "Spotify ID" }).click();
  await page.keyboard.press("Escape");
  await expect.poll(async () => (await saved()).hidden).not.toContain("sp_id");
  const before = await saved();
  expect(before.title.flex).toBeUndefined();
  expect(before.title.width).toBeGreaterThan(200);
  expect(before.plays.width).toBe(160);

  for (const width of [390, 1440]) {
    await page.setViewportSize({ width, height: 844 });
    // The compact layout narrows the controls column...
    await expect
      .poll(async () => widthOf("yt_id"))
      .toBe(width === 390 ? 120 : 136);
    // ...and leaves the user's settings alone.
    expect(await saved()).toEqual(before);
  }
});

for (const width of [320, 390]) {
  test(`a sync conflict's resolve action stays readable at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 844 });
    const { send } = await openApp(page);
    send({
      type: "git-sync-status",
      state: "conflict-requires-attention",
      message: "Local and upstream changes conflict",
      retryable: false,
    });
    // The sync status, grouped with the collection menu, says what to do.
    const resolve = page
      .locator(".collection-controls")
      .getByRole("button", { name: "Resolve conflict…", exact: true });
    await expect(resolve).toBeInViewport();
    const box = (await resolve.boundingBox())!;
    expect(box.x + box.width).toBeLessThanOrEqual(width);
    // Its visible label is all there: not truncated.
    const label = resolve.getByText("Resolve conflict…", { exact: true });
    await expect(label).toBeVisible();
    expect(await label.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(
      true,
    );
    await resolve.click();
    await expect(
      page.getByRole("alertdialog", { name: "Resolve collection conflict" }),
    ).toBeVisible();
  });
}

test("the conflict dialog stays with its conflict, and sends nothing once it's over", async ({
  page,
}) => {
  const { messages, send } = await openApp(page);
  const status = (state: string, message: string, retryable = false) =>
    send({ type: "git-sync-status", state, message, retryable });
  const conflict = (message: string) =>
    status("conflict-requires-attention", message);
  const resolve = page
    .locator(".collection-controls")
    .getByRole("button", { name: "Resolve conflict…", exact: true });
  const dialog = page.getByRole("alertdialog", {
    name: "Resolve collection conflict",
  });
  const resolutions = () =>
    messages.filter((message) => message.type === "resolve-git-conflict");

  // Edits (a play count, say) report local changes, which don't settle it: it stays
  // shown and its dialog open, also while a sync finds the conflict again.
  conflict("Local and upstream changes conflict");
  await resolve.click();
  await expect(dialog).toBeVisible();
  status("local-changes", "Changes will be synced within 5 minutes", true);
  await page.waitForTimeout(200);
  await expect(dialog).toBeVisible();
  await expect(resolve).toBeVisible();
  status("syncing", "Saving collection changes");
  await page.waitForTimeout(200);
  await expect(dialog).toBeVisible();
  conflict("Local and upstream changes conflict");
  await page.waitForTimeout(200);
  await expect(dialog).toBeVisible();
  await expect(resolve).toBeVisible();

  // A sync that ends the conflict closes it.
  status("syncing", "Saving collection changes");
  status("synced", "Repository is synchronized");
  await expect(dialog).toBeHidden();
  await expect(resolve).toHaveCount(0);

  // So does another conflict, and the first one coming back doesn't reopen it.
  conflict("Local and upstream changes conflict");
  await resolve.click();
  await expect(dialog).toBeVisible();
  conflict("Another conflict");
  await expect(dialog).toBeHidden();
  conflict("Local and upstream changes conflict");
  await expect(resolve).toBeVisible();
  await expect(dialog).toBeHidden();
  expect(resolutions()).toEqual([]);

  // The current conflict's dialog still resolves it.
  await resolve.click();
  await dialog
    .getByRole("button", { name: "Use upstream", exact: true })
    .click();
  await expect(dialog).toBeHidden();
  await expect.poll(resolutions).toEqual([
    expect.objectContaining({
      type: "resolve-git-conflict",
      strategy: "keep-upstream",
    }),
  ]);
});

test("a background sync's diverged histories offer a retry, not a resolution", async ({
  page,
}) => {
  const { messages, send } = await openApp(page);
  send({
    type: "git-sync-status",
    state: "conflict-requires-attention",
    message:
      "Local and upstream histories diverged; retry to reconcile them safely",
    retryable: true,
  });
  const controls = page.locator(".collection-controls");
  await controls
    .getByRole("button", { name: "Sync conflict: Retry sync", exact: true })
    .click();
  await expect(
    controls.getByRole("button", { name: "Resolve conflict…", exact: true }),
  ).toHaveCount(0);
  await expect
    .poll(() => messages.filter((message) => message.type === "retry-git-sync"))
    .toHaveLength(1);
});

test("first-use and empty collection states offer next actions", async ({
  page,
}) => {
  await openApp(page, { noCollection: true });
  await expect(
    page.getByRole("heading", { name: "Create your first collection" }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Create collection", exact: true })
    .click();
  await expect(page.getByRole("dialog")).toBeVisible();
});

test("collections and repositories are named without their Windows paths", async ({
  page,
}) => {
  await openApp(page, { windowsPaths: true });
  const menuButton = page
    .locator(".collection-controls")
    .getByRole("button")
    .first();
  await expect(menuButton).toHaveText("collection");
  await menuButton.click();
  await expect(
    page.getByRole("menuitemradio", { name: "collection", exact: true }),
  ).toBeVisible();
  await page.getByRole("menuitem", { name: "Set remote…" }).click();
  await expect(
    page.getByRole("dialog", { name: "Remote for audit" }),
  ).toBeVisible();
});

test("a backslash in a Unix collection's file name is part of its name", async ({
  page,
}) => {
  await openApp(page, {
    extraCollections: ["/audit/A\\House.db", "/audit/House.db"],
  });
  await page
    .locator(".collection-controls")
    .getByRole("button")
    .first()
    .click();
  const items = page
    .getByRole("group", { name: "Collections" })
    .getByRole("menuitemradio");
  await expect(items).toHaveText(["collection", "A\\House", "House"]);
});

test("empty collections offer search and import", async ({ page }) => {
  await openApp(page, { empty: true });
  await expect(
    page.getByRole("heading", { name: "Your collection is empty" }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Search for music", exact: true })
    .click();
  await expect(
    page.getByRole("textbox", { name: /Search query/ }),
  ).toBeFocused();
});

test("the collection menu sets, changes and removes the repository's remote", async ({
  page,
}) => {
  const { messages, send } = await openApp(page);
  const remoteUpdates = () =>
    messages.filter((message) => message.type === "set-git-remote");
  const lastUpdate = () => remoteUpdates().at(-1) as { id: number };
  const openMenu = () =>
    page.locator(".collection-controls").getByRole("button").first().click();
  const dialog = page.getByRole("dialog", { name: "Remote for audit" });
  const url = dialog.getByRole("textbox", { name: /Remote URL/ });

  // Enter saves, for the repository selected, and the dialog waits for the backend.
  await openMenu();
  await page.getByRole("menuitem", { name: "Set remote…" }).click();
  await expect(url).toHaveValue("");
  await url.fill("git@example.com:me/se");
  // The Enter that commits an input method editor's text doesn't save.
  await url.dispatchEvent("keydown", composingEnter);
  await expect(dialog).toBeVisible();
  await url.fill("git@example.com:me/sets.git");
  await url.press("Enter");
  await expect.poll(remoteUpdates).toEqual([
    expect.objectContaining({
      repo: "/audit",
      url: "git@example.com:me/sets.git",
    }),
  ]);
  await expect(dialog).toBeVisible();
  send({
    type: "set-git-remote-reply",
    id: lastUpdate().id,
    url: "git@example.com:me/sets.git",
  });
  await expect(dialog).toHaveCount(0);

  // A replacement that fails keeps the dialog open with the draft.
  send({
    type: "repos",
    repos: [{ path: "/audit", remote: "git@example.com:me/sets.git" }],
  });
  await openMenu();
  await page.getByRole("menuitem", { name: "Change remote…" }).click();
  await expect(url).toHaveValue("git@example.com:me/sets.git");
  await expect(dialog.getByRole("button", { name: "Save" })).toBeDisabled();
  await url.fill("git@example.com:me/private-sets.git");
  await dialog.getByRole("button", { name: "Save" }).click();
  await expect.poll(() => remoteUpdates().length).toBe(2);
  expect(lastUpdate()).toMatchObject({
    repo: "/audit",
    url: "git@example.com:me/private-sets.git",
  });
  send({ type: "error-reply", id: lastUpdate().id, message: "no access" });
  await expect(dialog.getByRole("button", { name: "Save" })).toBeEnabled();
  await expect(url).toHaveValue("git@example.com:me/private-sets.git");

  await dialog.getByRole("button", { name: "Remove" }).click();
  await expect.poll(() => remoteUpdates().length).toBe(3);
  expect(lastUpdate()).toMatchObject({ repo: "/audit", url: null });
  send({ type: "set-git-remote-reply", id: lastUpdate().id, url: null });
  await expect(dialog).toHaveCount(0);
});

test("Enter in the clone dialog clones the repository", async ({ page }) => {
  const { messages } = await openApp(page);
  await page
    .locator(".collection-controls")
    .getByRole("button")
    .first()
    .click();
  await page.getByRole("menuitem", { name: "Clone repository…" }).click();
  const dialog = page.getByRole("dialog", { name: "Clone a repository" });
  const url = dialog.getByRole("textbox", { name: "Git URL" });
  const clones = () =>
    messages.filter((message) => message.type === "clone-repo");
  await url.fill("https://example.com/me/se");
  // The Enter that commits an input method editor's text doesn't clone.
  await url.dispatchEvent("keydown", composingEnter);
  await expect(dialog).toBeVisible();
  await url.fill("https://example.com/me/sets.git");
  await url.press("Enter");
  await expect(dialog).toHaveCount(0);
  await expect
    .poll(clones)
    .toEqual([
      expect.objectContaining({ url: "https://example.com/me/sets.git" }),
    ]);
});

/** The playing row's progress, as set on its first element. */
const rowProgress = (page: Page, rowIndex: number) =>
  page
    .locator(`.ag-row.progress-bar[row-index="${rowIndex}"]`)
    .first()
    .evaluate((row) =>
      Number(row.style.getPropertyValue("--stereo-playback-progress")),
    );

/**
 * Lets the test stop the app's once-a-second playback updates (`setInterval`
 * with 1000 ms: the row progress and the footer's position) with
 * `window.__holdProgress`, so what it sees can't come from one of those.
 */
async function holdableProgressUpdates(page: Page) {
  await page.addInitScript(() => {
    const flags = window as unknown as { __holdProgress?: boolean };
    const setInterval = window.setInterval;
    window.setInterval = ((handler: TimerHandler, ms?: number) =>
      setInterval(() => {
        if (ms === 1000 && flags.__holdProgress) return;
        if (typeof handler === "function") handler();
      }, ms)) as typeof window.setInterval;
  });
}

const holdProgress = (page: Page) =>
  page.evaluate(() => {
    (window as unknown as { __holdProgress?: boolean }).__holdProgress = true;
  });

test(
  "the playing row shows how far the track has played",
  { tag: "@webkit" },
  async ({ page }) => {
    await openApp(page, { audio: "ok", audioSeconds: 4 });
    const sheets = await page.evaluate(
      () => document.adoptedStyleSheets.length,
    );
    await startPlayback(page);
    // Set on the row itself: WebKit restyles the whole page for a stylesheet change.
    await expect
      .poll(() => rowProgress(page, 0), { timeout: 5000 })
      .toBeGreaterThan(0);
    expect(await page.evaluate(() => document.adoptedStyleSheets.length)).toBe(
      sheets,
    );
  },
);

test(
  "the playing row shows its progress when it's rendered again",
  { tag: "@webkit" },
  async ({ page }) => {
    await holdableProgressUpdates(page);
    await openApp(page, { audio: "ok", audioSeconds: 240, trackCount: 200 });
    await startPlayback(page);
    const slider = page.getByRole("slider", {
      name: "Playback position",
      exact: true,
    });
    await expect(slider).toBeEnabled();
    await page.locator(".play-pause-button").click();
    await expect(page.locator(".play-pause-button")).toHaveAccessibleName(
      "Play",
    );
    const before = Number(await slider.getAttribute("aria-valuenow"));
    await holdProgress(page);

    // Seeks while the playing row is scrolled out of the grid, and so not rendered.
    // The grid's rows scroll in whichever of its elements scrolls vertically.
    const scrollGrid = (top: number) =>
      page
        .locator(".grid")
        .first()
        .evaluate((grid, top) => {
          const scroller = [...grid.querySelectorAll("*")].find(
            (element) =>
              element.scrollHeight > element.clientHeight + 100 &&
              getComputedStyle(element).overflowY !== "visible",
          );
          scroller!.scrollTop = top;
        }, top);
    const playingRow = page.locator('.ag-row[row-index="0"]');
    await scrollGrid(1e6);
    await expect(playingRow).toHaveCount(0);
    await slider.focus();
    await page.keyboard.press("ArrowRight");
    await expect
      .poll(async () => Number(await slider.getAttribute("aria-valuenow")))
      .toBeGreaterThan(before + 4);
    const after = Number(await slider.getAttribute("aria-valuenow"));
    await scrollGrid(0);
    await expect(playingRow.first()).toBeVisible();
    // Where it was sought to, not where it was when last rendered (before).
    expect(await rowProgress(page, 0)).toBeCloseTo(after / 240, 2);
  },
);

test(
  "a track skipped to starts its row's progress from the start",
  { tag: "@webkit" },
  async ({ page }) => {
    const { setAudio } = await openApp(page, {
      audio: "ok",
      audioSeconds: 30,
    });
    await startPlayback(page);
    const slider = page.getByRole("slider", {
      name: "Playback position",
      exact: true,
    });
    await expect(slider).toBeEnabled();
    await slider.focus();
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("ArrowRight");
    await expect
      .poll(() => rowProgress(page, 0))
      .toBeGreaterThanOrEqual(10 / 30);
    // The next track's audio doesn't load, so it never plays.
    setAudio("hold");
    await page
      .getByRole("button", { name: "Skip to next track", exact: true })
      .click();
    await expect(
      page.locator('.ag-row.progress-bar[row-index="1"]').first(),
    ).toBeVisible();
    expect(await rowProgress(page, 1)).toBe(0);
  },
);

test("unloaded rows do not expose ratings or playback highlights", async ({
  page,
}) => {
  await openApp(page, { holdRows: true });
  await expect(page.locator(".grid")).toBeVisible();
  await expect(page.locator(".grid").getByRole("radiogroup")).toHaveCount(0);
  await expect(page.locator(".grid .progress-bar")).toHaveCount(0);
});

test("reduced motion disables decorative animation", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openApp(page);
  const animation = await page
    .locator(".background-player")
    .evaluate((element) => getComputedStyle(element).animationName);
  expect(animation).toBe("none");
});

/** The disco ball's canvas, as drawn: changes whenever it's redrawn differently. */
const discoBallPixels = (page: Page) =>
  page
    .locator("canvas.disco-ball")
    .evaluate((c: HTMLCanvasElement) => c.toDataURL());

/** Sets one of the disco ball's sliders in the settings menu with the keyboard. */
async function setDiscoBallSlider(page: Page, name: string, key: string) {
  await page.getByRole("button", { name: "Settings" }).click();
  const slider = page.getByRole("slider", { name });
  await slider.focus();
  await slider.press(key);
  await page.keyboard.press("Escape");
}

test("the disco ball is hidden unless asked for", async ({ page }) => {
  await openApp(page);
  await expect(page.locator("canvas.disco-ball")).toHaveCount(0);
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("switch", { name: "Disco ball" }).click();
  await expect(page.locator("canvas.disco-ball")).toBeVisible();
});

test("the disco ball hangs still unless asked to turn", async ({ page }) => {
  await openApp(page, { discoBall: true });
  const still = await discoBallPixels(page);
  await page.waitForTimeout(300);
  expect(await discoBallPixels(page)).toBe(still);

  await setDiscoBallSlider(page, "Rotation speed", "End");
  await expect
    .poll(() => discoBallPixels(page), { timeout: 2000 })
    .not.toBe(still);
});

/** The settings as stored, parsed. */
const storedSettings = (page: Page) =>
  page.evaluate(() => JSON.parse(localStorage.getItem("stereo_app_settings")!));

test("the visual effects reset to their defaults", async ({ page }) => {
  await openApp(page);
  // Hiding the background leaves its sliders disabled: the reset shows it again.
  await page.getByRole("button", { name: "Settings" }).click();
  const backgroundToggle = page.getByRole("switch", {
    name: "Background artwork",
  });
  await backgroundToggle.click();
  await expect(backgroundToggle).not.toBeChecked();
  const hint = page.getByText(
    "Resetting also shows the background artwork again.",
  );
  await expect(hint).toBeVisible();
  const reset = page.getByRole("button", { name: "Reset visual effects" });
  await reset.click();
  await expect(backgroundToggle).toBeChecked();
  await expect(hint).toBeHidden();
  await expect(reset).toBeDisabled();

  // Set back to its default, a setting is still stored: there's still something to reset.
  const blur = page.getByRole("slider", { name: "Background blur" });
  await blur.press("End");
  for (let i = 0; i < 20; i++) await blur.press("ArrowLeft");
  await expect(blur).toHaveAttribute("aria-valuenow", "100");
  await expect(reset).toBeEnabled();

  await page.getByRole("switch", { name: "Disco ball" }).click();
  const size = page.getByRole("slider", { name: "Ball size" });
  await size.press("End");
  await expect(size).toHaveAttribute("aria-valuenow", "100");
  await expect(page.locator("canvas.disco-ball")).toBeVisible();

  await reset.click();
  await expect(size).toHaveAttribute("aria-valuenow", "40");
  await expect(reset).toBeDisabled();
  await expect(page.locator("canvas.disco-ball")).toHaveCount(0);
  const stored = await storedSettings(page);
  for (const field of [
    "backgroundVideo",
    "backgroundPulse",
    "background",
    "air",
    "discoBall",
  ])
    expect(stored).not.toHaveProperty(field);
});

test("a reset holds while another tab changes other settings", async ({
  context,
}) => {
  const first = await context.newPage();
  await openApp(first, { discoBall: true });
  const second = await context.newPage();
  await openApp(second);
  await expect(second.locator("canvas.disco-ball")).toBeVisible();

  await first.getByRole("button", { name: "Settings" }).click();
  await first.getByRole("button", { name: "Reset visual effects" }).click();
  await expect(first.locator("canvas.disco-ball")).toHaveCount(0);
  // The second tab still shows the ball: changing its shuffle stores just that.
  const shuffle = second.getByRole("button", { name: "Toggle shuffle play" });
  const shuffled = (await storedSettings(second)).shufflePlay;
  await shuffle.click();
  await expect
    .poll(async () => (await storedSettings(second)).shufflePlay)
    .toBe(!shuffled);
  expect(await storedSettings(second)).not.toHaveProperty("discoBall");
  await first.reload();
  await expect(first.locator(".collection-controls")).toBeVisible();
  await expect(first.locator("canvas.disco-ball")).toHaveCount(0);
});

test("settings stored with the old defaults have nothing to reset", async ({
  page,
}) => {
  await page.addInitScript(() => {
    if (localStorage.getItem("stereo_app_settings")) return;
    localStorage.setItem(
      "stereo_app_settings",
      JSON.stringify({
        appearance: "system",
        repoPath: "",
        collectionPath: "",
        backgroundVideo: true,
        backgroundPulse: 50,
        shufflePlay: false,
      }),
    );
  });
  await openApp(page);
  await page.getByRole("button", { name: "Settings" }).click();
  await expect(
    page.getByRole("button", { name: "Reset visual effects" }),
  ).toBeDisabled();
  const stored = await storedSettings(page);
  expect(stored).not.toHaveProperty("backgroundVideo");
  expect(stored).not.toHaveProperty("backgroundPulse");
});

test("the disco ball fits a short window, which keeps its cord setting", async ({
  page,
}) => {
  await page.setViewportSize({ width: 667, height: 375 });
  await openApp(page, { discoBall: true });
  await setDiscoBallSlider(page, "Cord length", "End");
  expect(
    await page.evaluate(
      () =>
        JSON.parse(localStorage.getItem("stereo_app_settings")!).discoBall.cord,
    ),
  ).toBe(400);
  const ball = page.locator("canvas.disco-ball");
  await expect
    .poll(() => ball.evaluate((c) => c.getBoundingClientRect().bottom))
    .toBeLessThanOrEqual(375);
  // Room again, and it hangs as low as asked.
  await page.setViewportSize({ width: 1440, height: 900 });
  await expect
    .poll(() => ball.evaluate((c) => c.getBoundingClientRect().height))
    // The cord, the ring and cap (9px), the ball (40% of 900px) and its margin (4px).
    .toBe(400 + 9 + 360 + 4);
});

test("the disco ball shrinks below its smallest size to fit, then isn't drawn", async ({
  page,
}) => {
  await openApp(page, { discoBall: true });
  const ball = page.locator("canvas.disco-ball");
  await page.setViewportSize({ width: 667, height: 40 });
  await expect
    .poll(() => ball.evaluate((c) => c.getBoundingClientRect().bottom))
    .toBeLessThanOrEqual(40);
  await expect(ball).toBeVisible();
  await page.setViewportSize({ width: 667, height: 20 });
  await expect(ball).toBeHidden();
  await page.setViewportSize({ width: 667, height: 375 });
  await expect(ball).toBeVisible();
});

test("the disco ball is fainter behind a grid, and behind centred content it reaches", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openApp(page, { empty: true, discoBall: true });
  const ball = page.locator("canvas.disco-ball");
  // It ends above the empty state's text.
  await expect(ball).not.toHaveClass(/disco-ball-subdued/);
  await page.setViewportSize({ width: 667, height: 375 });
  await expect(ball).toHaveClass(/disco-ball-subdued/);
});

test("the disco ball is fainter behind the collection's rows", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openApp(page, { discoBall: true });
  await expect(page.locator("canvas.disco-ball")).toHaveClass(
    /disco-ball-subdued/,
  );
});

test("motion settings say they're off for those who prefer reduced motion", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openApp(page, { discoBall: true });
  await page.getByRole("button", { name: "Settings" }).click();
  for (const name of ["Rotation speed", "Background pulse", "Wave duration"])
    await expect(page.getByRole("slider", { name })).toBeDisabled();
  await expect(
    page.getByRole("switch", { name: "Speaker air" }),
  ).toBeDisabled();
  // Not the others.
  await expect(page.getByRole("slider", { name: "Ball size" })).toBeEnabled();
});

test("the disco ball is redrawn for a display of another pixel density", async ({
  page,
}) => {
  // Moving the window to a display with another scale changes the ratio without
  // resizing it; the page hears of it through a media query on the old ratio.
  // (Chromium's emulation of another scale tells pages neither way.)
  await page.addInitScript(() => {
    const w = window as unknown as {
      testRatio: number;
      testRatioChanged: () => void;
    };
    w.testRatio = 1;
    const listeners = new Set<() => void>();
    w.testRatioChanged = () => listeners.forEach((listener) => listener());
    Object.defineProperty(window, "devicePixelRatio", {
      get: () => w.testRatio,
    });
    const matchMedia = window.matchMedia.bind(window);
    window.matchMedia = (query: string) => {
      if (!query.startsWith("(resolution:")) return matchMedia(query);
      const list = matchMedia("(min-width: 0px)");
      list.addEventListener = (_type: string, listener: unknown) =>
        void listeners.add(listener as () => void);
      list.removeEventListener = (_type: string, listener: unknown) =>
        void listeners.delete(listener as () => void);
      return list;
    };
  });
  await openApp(page, { discoBall: true });
  const ball = page.locator("canvas.disco-ball");
  const bitmapScale = () =>
    ball.evaluate(
      (c: HTMLCanvasElement) => c.width / c.getBoundingClientRect().width,
    );
  await expect.poll(bitmapScale).toBeCloseTo(1, 1);
  for (const ratio of [2, 3, 1.5]) {
    await page.evaluate((ratio) => {
      const w = window as unknown as {
        testRatio: number;
        testRatioChanged: () => void;
      };
      w.testRatio = ratio;
      w.testRatioChanged();
    }, ratio);
    await expect.poll(bitmapScale).toBeCloseTo(ratio, 1);
  }
});

test("metadata editing groups identifiers and explains invalid required fields", async ({
  page,
}) => {
  await openApp(page);
  await page
    .getByRole("button", { name: "Options for Example track 1", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Edit", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(
    dialog.getByRole("button", { name: "Fill missing metadata" }),
  ).toBeVisible();
  await expect(
    dialog.getByRole("textbox", { name: "Spotify ID", exact: true }),
  ).toBeHidden();
  const title = dialog.getByRole("textbox", { name: /^Title/ });
  await title.fill("");
  await expect(title).toHaveAccessibleDescription(/Required/);
  await expect(
    dialog.getByRole("button", { name: "Save", exact: true }),
  ).toBeDisabled();
});

test("a validation reply for an abandoned edit doesn't enable saving after reopening", async ({
  page,
}) => {
  const { messages, send } = await openApp(page, { holdValidation: true });
  const openEdit = async () => {
    await page
      .getByRole("button", { name: "Options for Example track 1", exact: true })
      .click();
    await page.getByRole("menuitem", { name: "Edit", exact: true }).click();
  };
  await openEdit();
  const dialog = page.getByRole("dialog");
  const title = dialog.getByRole("textbox", { name: /^Title/ });
  await title.fill("Edited title");
  await expect
    .poll(() => messages.filter((m) => m.type === "validate-track").length)
    .toBeGreaterThan(0);
  const validation = messages.findLast((m) => m.type === "validate-track")!;
  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(dialog).toHaveCount(0);

  await openEdit();
  await expect(title).toHaveValue("Example track 1");
  send({ type: "validate-track-reply", id: validation.id, is_valid: true });
  await page.waitForTimeout(300);
  await expect(
    dialog.getByRole("button", { name: "Save", exact: true }),
  ).toBeDisabled();
});

test("the collection grid keeps this tab's view after a search, whatever another tab saved", async ({
  page,
}) => {
  await openApp(page);
  const titleHeader = page.locator('.ag-header-cell[col-id="title"]');
  await titleHeader.getByText("Title", { exact: true }).click();
  await expect(titleHeader).toHaveAttribute("aria-sort", "ascending");
  // Another tab filters the collection down to nothing.
  await page.evaluate(() =>
    localStorage.setItem(
      "stereo_app_grid_state_v2",
      JSON.stringify({
        filter: {
          filterModel: {
            title: { filterType: "text", type: "contains", filter: "none" },
          },
        },
      }),
    ),
  );

  await page
    .getByRole("textbox", { name: /Search query/ })
    .fill("missing track");
  await page.keyboard.press("Enter");
  await page
    .getByRole("button", { name: "Back to collection", exact: true })
    .click();
  await expect(
    page
      .locator('.ag-row[row-index="0"]')
      .getByText("Example track 1", { exact: true }),
  ).toBeVisible();
  await expect(titleHeader).toHaveAttribute("aria-sort", "ascending");
  await expect(
    page.getByRole("heading", { name: "No tracks match these filters" }),
  ).toHaveCount(0);
});

test("empty search results provide a way back to the collection", async ({
  page,
}) => {
  await openApp(page);
  await page
    .getByRole("textbox", { name: /Search query/ })
    .fill("missing track");
  await page.keyboard.press("Enter");
  await expect(
    page.getByRole("heading", { name: "No tracks found", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Back to collection", exact: true })
    .click();
  await expect(
    page
      .locator('.ag-row[row-index="0"]')
      .getByText("Example track 1", { exact: true }),
  ).toBeVisible();
});

test("filtered collections offer a working clear-filters action", async ({
  page,
}) => {
  await openApp(page);
  await page
    .locator('.ag-header-cell[col-id="title"] .ag-header-cell-filter-button')
    .click();
  await page
    .getByRole("textbox", { name: "Filter Value", exact: true })
    .fill("no match");
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("heading", {
      name: "No tracks match these filters",
      exact: true,
    }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Clear filters", exact: true })
    .click();
  await expect(
    page
      .locator('.ag-row[row-index="0"]')
      .getByText("Example track 1", { exact: true }),
  ).toBeVisible();
});

test("resetting filters and sorting keeps the displayed columns", async ({
  page,
}) => {
  const { messages } = await openApp(page);
  const clear = page.getByRole("button", {
    name: "Reset filters and sorting",
    exact: true,
  });
  await expect(clear).toHaveCount(0);

  await page.getByRole("button", { name: "View", exact: true }).click();
  await page.getByRole("menuitemcheckbox", { name: "Genre" }).click();
  await page.keyboard.press("Escape");
  const genreHeader = page.locator('.ag-header-cell[col-id="genre"]');
  await expect(genreHeader).toHaveCount(0);

  const titleHeader = page.locator('.ag-header-cell[col-id="title"]');
  await titleHeader.getByText("Title", { exact: true }).click();
  await expect(titleHeader).toHaveAttribute("aria-sort", "ascending");
  await titleHeader.locator(".ag-header-cell-filter-button").click();
  await page
    .getByRole("textbox", { name: "Filter Value", exact: true })
    .fill("no match");
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("heading", { name: "No tracks match these filters" }),
  ).toBeVisible();

  // From the keyboard, focus moves on to the View menu as the button goes away.
  const before = messages.length;
  await clear.focus();
  await page.keyboard.press("Enter");
  await expect(
    page
      .locator('.ag-row[row-index="0"]')
      .getByText("Example track 1", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "View", exact: true }),
  ).toBeFocused();
  // One page query, for the final view, not one per change (besides the playback
  // queue's refresh, which asks for up to 10000 rows).
  expect(
    messages
      .slice(before)
      .filter(
        (message) => message.type === "get-rows" && message.endRow === 100,
      ),
  ).toEqual([
    expect.objectContaining({
      filterModel: {},
      sortModel: [
        expect.objectContaining({ colId: "release_date", sort: "desc" }),
      ],
    }),
  ]);
  await expect(titleHeader).toHaveAttribute("aria-sort", "none");
  await expect(
    page.locator('.ag-header-cell[col-id="release_date"]'),
  ).toHaveAttribute("aria-sort", "descending");
  await expect(genreHeader).toHaveCount(0);
  await expect(clear).toHaveCount(0);
});

test("a sort restored from the last session can be reset", async ({ page }) => {
  await page.addInitScript(() => {
    if (localStorage.getItem("stereo_app_grid_state_v2")) return;
    localStorage.setItem(
      "stereo_app_grid_state_v2",
      JSON.stringify({
        sort: { sortModel: [{ colId: "title", sort: "asc" }] },
      }),
    );
  });
  await openApp(page);
  const titleHeader = page.locator('.ag-header-cell[col-id="title"]');
  await expect(titleHeader).toHaveAttribute("aria-sort", "ascending");
  await page
    .getByRole("button", { name: "Reset filters and sorting", exact: true })
    .click();
  await expect(titleHeader).toHaveAttribute("aria-sort", "none");
  await expect(
    page.locator('.ag-header-cell[col-id="release_date"]'),
  ).toHaveAttribute("aria-sort", "descending");
});

test("grid arrow keys move between rows without skipping playback", async ({
  page,
}) => {
  const { messages } = await openApp(page);
  const first = page.locator('.ag-row[row-index="0"] [col-id="title"]');
  await first.focus();
  const before = messages.filter(
    (message) => message.type === "get-track-info",
  ).length;
  await page.keyboard.press("ArrowDown");
  await expect(
    page.locator('.ag-row[row-index="1"] [col-id="title"]'),
  ).toBeFocused();
  expect(
    messages.filter((message) => message.type === "get-track-info"),
  ).toHaveLength(before);
});

test("single-key shortcuts still work after clicking a button or a grid row", async ({
  page,
}) => {
  await openApp(page);
  const shuffle = page.getByRole("button", {
    name: "Toggle shuffle play",
    exact: true,
  });
  await page.locator(".play-pause-button").click();
  const artwork = page.locator(".background-player");
  await expect(artwork).toHaveCount(1);
  await page.keyboard.press("b");
  await expect(artwork).toHaveCount(0);
  await page.locator('.ag-row[row-index="2"] [col-id="title"]').click();
  await page.keyboard.press("s");
  await expect(shuffle).toHaveAttribute("aria-pressed", "true");
});

test("waveform arrow keys seek five seconds instead of triggering global shortcuts", async ({
  page,
}) => {
  await openApp(page, { audio: "ok", audioSeconds: 240 });
  await startPlayback(page);
  const slider = page.getByRole("slider", {
    name: "Playback position",
    exact: true,
  });
  await expect(slider).toBeEnabled();
  await page.locator(".play-pause-button").click();
  await expect(page.locator(".play-pause-button")).toHaveAccessibleName("Play");
  const before = Number(await slider.getAttribute("aria-valuenow"));
  await slider.focus();
  await page.keyboard.press("ArrowRight");
  // One step is 5 seconds, and the arrow key doesn't also skip tracks.
  await expect
    .poll(async () => Number(await slider.getAttribute("aria-valuenow")))
    .toBeCloseTo(before + 5, 0);
  await expect(page.getByText("00:05", { exact: true })).toBeVisible();
});

test("connecting YouTube asks which login to use when several are logged in", async ({
  page,
}) => {
  const { messages } = await openApp(page);
  await page.getByRole("button", { name: /^YouTube:/ }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("button", { name: "Connect", exact: true }).click();
  await dialog.getByRole("radio", { name: /Work Channel/ }).check();
  await dialog.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(
    dialog.getByText(
      "Playlists are created as Work Channel (@work) in Firefox (work).",
    ),
  ).toBeVisible();
  const connects = messages.filter((m) => m.type === "connect-youtube-session");
  expect(connects.map((m) => m.login)).toEqual([
    undefined,
    "/p/work/cookies.sqlite",
  ]);
});

test("loading the page selects the first track without playing it", async ({
  page,
}) => {
  const { audioRequests } = await openApp(page, { audio: "ok" });
  await expect(page.locator('.ag-row[row-index="0"]')).toBeVisible();
  // Give a wrongly started track time to be requested.
  await page.waitForTimeout(500);
  expect(audioRequests).toEqual([]);
  await expect(page.locator(".play-pause-button")).toHaveAccessibleName("Play");
  await page.keyboard.press("p");
  await expect
    .poll(() => audioRequests)
    .toContain("GET /audio/audit0?key=test-key");
  await expect(page.locator(".play-pause-button")).toHaveAccessibleName(
    "Pause",
  );
});

test("plays downloaded audio, prefetching and advancing on its own", async ({
  page,
}) => {
  const { audioRequests } = await openApp(page, { audio: "ok" });
  await startPlayback(page);
  await expect
    .poll(() => audioRequests)
    .toContain("GET /audio/audit0?key=test-key");
  await expect(page.locator(".play-pause-button")).toHaveAccessibleName(
    "Pause",
  );
  await expect
    .poll(() => audioRequests)
    .toEqual(
      expect.arrayContaining([
        "HEAD /audio/audit1?key=test-key",
        "HEAD /audio/audit2?key=test-key",
      ]),
    );
  // The 1s track ends and the engine starts the next one itself.
  await expect
    .poll(() => audioRequests)
    .toContain("GET /audio/audit1?key=test-key");
});

test("skips tracks whose audio can't be downloaded, and stops after three in a row", async ({
  page,
}) => {
  const { audioRequests } = await openApp(page, { audio: "fail" });
  await startPlayback(page);
  await expect(
    toastText(
      page,
      "Couldn't download this track's audio, skipping it: yt-dlp failed",
      { exact: true },
    ).first(),
  ).toBeVisible();
  await expect(
    toastText(
      page,
      "Couldn't download the audio of 3 tracks in a row, stopping: yt-dlp failed",
      { exact: true },
    ),
  ).toBeVisible();
  await expect(page.locator(".play-pause-button")).toHaveAccessibleName("Play");
  const played = new Set(
    audioRequests
      .filter((r) => r.startsWith("GET "))
      .map((r) => r.split("?")[0]),
  );
  expect([...played].sort()).toEqual([
    "GET /audio/audit0",
    "GET /audio/audit1",
    "GET /audio/audit2",
  ]);
});

test("a track paused while it downloads stays paused if the download fails", async ({
  page,
}) => {
  const { audioRequests, heldAudio, setAudio } = await openApp(page);
  await startPlayback(page);
  await expect.poll(() => heldAudio.length).toBeGreaterThan(0);
  const button = page.locator(".play-pause-button");
  await button.click();
  await expect(button).toHaveAccessibleName("Play");
  setAudio("fail");
  for (const route of heldAudio.splice(0))
    await route.fulfill({ status: 502, body: "yt-dlp failed" });
  await expect(
    toastText(page, "Couldn't download this track's audio: yt-dlp failed", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(button).toHaveAccessibleName("Play");
  expect(audioRequests.filter((r) => !r.includes("/audit0?"))).toEqual([]);
});

test("play retries a track whose download failed", async ({ page }) => {
  const { audioRequests, setAudio } = await openApp(page, {
    audio: "fail",
    audioSeconds: 240,
  });
  await startPlayback(page);
  await expect(
    toastText(
      page,
      "Couldn't download the audio of 3 tracks in a row, stopping: yt-dlp failed",
      { exact: true },
    ),
  ).toBeVisible();
  const button = page.locator(".play-pause-button");
  await expect(button).toHaveAccessibleName("Play");
  setAudio("ok");
  const before = audioRequests.length;
  await button.click();
  await expect(button).toHaveAccessibleName("Pause");
  expect(audioRequests.slice(before)).toContain(
    "GET /audio/audit2?key=test-key",
  );
});

test("shows the playing track's thumbnail, blurred, behind the app", async ({
  page,
}) => {
  await openApp(page);
  const background = page.locator(".background-player");
  await expect(background).toHaveCSS(
    "background-image",
    'url("https://i.ytimg.com/vi/audit0/mqdefault.jpg")',
  );
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("switch", { name: "Background artwork" }).click();
  await expect(background).toHaveCount(0);
});

test(
  "hovering the playing track's thumbnail shows it large, centered",
  { tag: "@webkit" },
  async ({ page }) => {
    await openApp(page);
    await page.route(
      "https://i.ytimg.com/vi/audit0/maxresdefault.jpg",
      (route) =>
        route.fulfill({
          contentType: "image/svg+xml",
          body: '<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360"/>',
        }),
    );
    await page
      .getByRole("img", { name: "Thumbnail of the playing track" })
      .hover();
    const card = page.getByRole("dialog", {
      name: "Thumbnail of the playing track",
    });
    await expect(card).toBeVisible();
    // Measured once it has finished scaling in.
    await card.evaluate((el) =>
      Promise.all(el.getAnimations({ subtree: true }).map((a) => a.finished)),
    );
    const box = (await card.boundingBox())!;
    const viewport = page.viewportSize()!;
    expect(box.width).toBeCloseTo(640, 0);
    expect(box.x + box.width / 2).toBeCloseTo(viewport.width / 2, 0);
    expect(box.y + box.height / 2).toBeCloseTo(viewport.height / 2, 0);
  },
);

test(
  "the large thumbnail stays closed after Escape though it covered the thumbnail",
  { tag: "@webkit" },
  async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await openApp(page);
    await page.route(
      "https://i.ytimg.com/vi/audit0/maxresdefault.jpg",
      (route) =>
        route.fulfill({
          contentType: "image/svg+xml",
          body: '<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720"/>',
        }),
    );
    const thumbnail = page.getByRole("img", {
      name: "Thumbnail of the playing track",
    });
    const card = page.getByRole("dialog", {
      name: "Thumbnail of the playing track",
    });
    await thumbnail.hover();
    await expect(card).toBeVisible();
    await card.evaluate((el) =>
      Promise.all(el.getAnimations({ subtree: true }).map((a) => a.finished)),
    );
    // The large thumbnail covers the pointer.
    const trigger = (await thumbnail.boundingBox())!;
    const box = (await card.boundingBox())!;
    const [x, y] = [
      trigger.x + trigger.width / 2,
      trigger.y + trigger.height / 2,
    ];
    expect(x > box.x && x < box.x + box.width).toBe(true);
    expect(y > box.y && y < box.y + box.height).toBe(true);
    await page.keyboard.press("Escape");
    await expect(card).toBeHidden();
    // Past the hover delay, with the pointer still where it was.
    await page.waitForTimeout(1000);
    await expect(card).toBeHidden();
    // Leaving the thumbnail and coming back opens it again.
    await page.mouse.move(640, 100);
    await thumbnail.hover();
    await expect(card).toBeVisible();
  },
);

test("the speakers' clearance doesn't wrap the header where it would fit", async ({
  page,
}) => {
  const { send } = await openApp(page);
  const oneRow = () =>
    page.evaluate(() => {
      const top = (selector: string) =>
        document.querySelector(selector)!.getBoundingClientRect().top;
      return Math.abs(top(".collection-controls") - top(".nav-controls")) < 4;
    });
  const stackToMenu = () =>
    page.evaluate(
      () =>
        document.querySelector(".collection-group")!.getBoundingClientRect()
          .left -
        document
          .querySelector(".speaker-stack:not(.speaker-stack-mirrored)")!
          .getBoundingClientRect().right,
    );
  // Below its breakpoint there's no clearance, and the header fits as before.
  await page.setViewportSize({ width: 990, height: 800 });
  expect(await oneRow()).toBe(true);
  expect(await stackToMenu()).toBe(8);
  // From it, the clearance fits even with the widest sync status beside the
  // Tailscale button.
  await page.setViewportSize({ width: 1280, height: 800 });
  send({
    type: "tailscale",
    status: { state: "serving", url: "https://stereo.example.ts.net" },
  });
  send({
    type: "git-sync-status",
    state: "authentication-required",
    message: "Authentication required",
    retryable: true,
  });
  await expect(
    page.getByRole("button", {
      name: "Authentication required: Retry sync",
      exact: true,
    }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: /^Tailnet/ })).toBeVisible();
  expect(await oneRow()).toBe(true);
  expect(await stackToMenu()).toBe(24);
});

test("the rotating background covers the window's corners, even without blur", async ({
  page,
}) => {
  await openApp(page);
  const background = page.locator(".background-player");
  await expect(background).toHaveCount(1);
  // Window shapes from portrait phones to ultra-wide monitors.
  for (const [width, height] of [
    [320, 844],
    [390, 844],
    [844, 320],
    [1000, 1000],
    [1440, 900],
    [3440, 1440],
    [3840, 1080],
    [1920, 400],
    [300, 1300],
    [2000, 200],
  ]) {
    await page.setViewportSize({ width, height });
    const uncovered = await background.evaluate((element: HTMLElement) => {
      // Without blur there's no margin past the window, and without beats no swelling:
      // at the animation's start it is scaled least and rotated most.
      element.style.setProperty("--background-blur", "0");
      for (const animation of element.getAnimations()) {
        animation.pause();
        animation.currentTime = 0;
      }
      const style = getComputedStyle(element);
      const matrix = new DOMMatrix(style.transform).multiply(
        new DOMMatrix(`scale(${style.scale === "none" ? 1 : style.scale})`),
      );
      const inverse = matrix.inverse();
      const [left, top] = [element.offsetLeft, element.offsetTop];
      const [w, h] = [element.offsetWidth, element.offsetHeight];
      const [cx, cy] = [left + w / 2, top + h / 2];
      return [
        [0, 0],
        [innerWidth, 0],
        [0, innerHeight],
        [innerWidth, innerHeight],
      ].filter(([x, y]) => {
        const p = inverse.transformPoint(new DOMPoint(x - cx, y - cy));
        return Math.abs(p.x) > w / 2 || Math.abs(p.y) > h / 2;
      });
    });
    expect(uncovered, `${width}×${height}`).toEqual([]);
  }
});

test("the logo builds up into a speaker stack with the volume, without moving the header", async ({
  page,
}) => {
  await openApp(page);
  const stack = page.getByRole("img", { name: "Stereo" });
  const cabinets = stack.locator("svg > g");
  // Its mirror image at the other end of the header: a stereo pair.
  const mirrored = page.locator(".speaker-stack-mirrored");
  const mirroredCabinets = mirrored.locator("svg > g");
  const slider = page.getByRole("slider", { name: "Volume" });
  const mute = page.getByRole("button", { name: "Mute" });
  const collection = page.locator(".collection-controls > :nth-child(2)");
  const collectionLeft = async () => (await collection.boundingBox())!.x;

  // Full volume by default: the logo's cabinet, two more beside it, two bass bins under them.
  await expect(cabinets).toHaveCount(5);
  await expect(mirroredCabinets).toHaveCount(5);
  await expect(mirrored).toHaveCSS("scale", "-1 1");
  const viewport = page.viewportSize()!;
  const [leftBox, rightBox] = [
    (await stack.boundingBox())!,
    (await mirrored.boundingBox())!,
  ];
  expect(leftBox.y).toBe(rightBox.y);
  expect(leftBox.x).toBeCloseTo(
    viewport.width - rightBox.x - rightBox.width,
    0,
  );
  const left = await collectionLeft();
  // However big the stack, it keeps the logo's box in the layout.
  expect(leftBox.width).toBe(28);
  expect(leftBox.height).toBe(28);

  await slider.focus();
  await page.keyboard.press("Home");
  await expect(cabinets).toHaveCount(1);
  await expect(mirroredCabinets).toHaveCount(1);
  expect(await collectionLeft()).toBe(left);

  await page.keyboard.press("End");
  await expect(cabinets).toHaveCount(5);
  await mute.click();
  await expect(cabinets).toHaveCount(1);
  await mute.click();
  await expect(cabinets).toHaveCount(5);

  // Behind the app's content.
  for (const s of [stack, mirrored]) {
    await expect(s).toHaveCSS("z-index", "-1");
    await expect(s).toHaveCSS("pointer-events", "none");
  }
  // The controls they run under are frosted over, to stay readable.
  await expect(page.getByRole("button", { name: "Settings" })).toHaveCSS(
    "backdrop-filter",
    "blur(8px)",
  );
});

test("offers only mute where the page can't set the volume (iOS)", async ({
  page,
}) => {
  // WebKit on iOS keeps an assigned volume for now and restores the old one in
  // a task it queues, so reading it back right away says it worked.
  await page.addInitScript(() => {
    const native = Object.getOwnPropertyDescriptor(
      HTMLMediaElement.prototype,
      "volume",
    )!;
    const assigned = new WeakMap<HTMLMediaElement, number>();
    Object.defineProperty(HTMLMediaElement.prototype, "volume", {
      configurable: true,
      get(this: HTMLMediaElement) {
        return assigned.get(this) ?? (native.get!.call(this) as number);
      },
      set(this: HTMLMediaElement, volume: number) {
        assigned.set(this, volume);
        setTimeout(() => assigned.delete(this));
      },
    });
  });
  await openApp(page);
  const mute = page.getByRole("button", { name: "Mute" });
  await expect(mute).toBeVisible();
  // Longer than the app gives a reversion to land.
  await page.waitForTimeout(1000);
  await expect(page.getByRole("slider", { name: "Volume" })).toHaveCount(0);
  // The stacks don't claim a volume that isn't applied: they're the logo alone.
  await expect(
    page.getByRole("img", { name: "Stereo" }).locator("svg > g"),
  ).toHaveCount(1);
  await expect(page.locator(".speaker-stack-mirrored svg > g")).toHaveCount(1);

  await mute.click();
  await expect(mute).toHaveAttribute("aria-pressed", "true");
});

test("explains that playback needs yt-dlp when the backend can't download audio", async ({
  page,
}) => {
  await openApp(page, { audio: "off" });
  await expect(
    page.getByText("Playback needs yt-dlp", { exact: true }),
  ).toBeVisible();
});

for (const banner of [false, true])
  test(`under the macOS desktop app's title bar, its strip stays clear${banner ? ", above a banner" : ""}`, async ({
    page,
  }) => {
    await openApp(page, { audio: banner ? "off" : "hold" });
    // What desktop/src-tauri/src/title_bar.rs sets while the title bar is over the page
    // (WebKit runs the app's script with the root element there, Chromium before it is).
    await page.evaluate(() => {
      document.documentElement.dataset.titleBar = "overlay";
    });
    const top = async (locator: Locator) => (await locator.boundingBox())!.y;
    const header = page.locator(".app-header");
    // The first thing under the strip: the banner, when there is one, else the header.
    const first = banner
      ? page.getByText("Playback needs yt-dlp", { exact: true })
      : header;
    await expect(first).toBeVisible();
    expect(await top(first)).toBeGreaterThanOrEqual(28);
    expect(await top(header)).toBeGreaterThanOrEqual(28);
    // Still exactly as tall as the window: nothing scrolls the strip away.
    const [scrollHeight, innerHeight] = await page.evaluate(() => [
      document.scrollingElement!.scrollHeight,
      window.innerHeight,
    ]);
    expect(scrollHeight).toBe(innerHeight);
    const footer = page.getByRole("button", { name: "Play", exact: true });
    expect((await footer.boundingBox())!.y).toBeLessThan(innerHeight);

    // In full screen there is no title bar until it slides in, and no strip.
    await page.evaluate(() => {
      document.documentElement.dataset.titleBar = "hidden";
    });
    await expect.poll(() => top(first)).toBeLessThan(28);
  });

test("shows the playing track's download progress in its waveform, and each track's in its row", async ({
  page,
}) => {
  const { send, audioRequests } = await openApp(page);
  await startPlayback(page);
  await expect
    .poll(() => audioRequests)
    .toContain("GET /audio/audit0?key=test-key");
  const waveform = page.getByRole("slider", { name: "Playback position" });
  const rowButton = (index: number) =>
    page.locator(`.ag-row[row-index="${index}"] .audio-cache-button`);
  await expect(waveform).toBeVisible();
  await expect(rowButton(0)).toHaveAccessibleName("Download");

  // Downloads of other tracks (prefetching) only show in their rows.
  send({
    type: "audio-download",
    yt_id: "audit1",
    status: "downloading",
    progress: 0.9,
  });
  await expect(rowButton(1)).toHaveAccessibleName("Downloading (90%)");
  await expect(page.getByText("90%", { exact: true })).toHaveCount(0);

  send({
    type: "audio-download",
    yt_id: "audit0",
    status: "preparing",
    progress: null,
  });
  await expect(page.getByText("Preparing", { exact: true })).toBeVisible();
  await expect(page.getByText("--%", { exact: true })).toBeVisible();
  // Pulsing until the size is known, and nothing to seek in yet.
  await expect(waveform).toHaveAttribute("data-pending", "true");
  await expect(waveform).toBeDisabled();
  await expect(rowButton(0)).toHaveAccessibleName("Preparing download…");

  send({
    type: "audio-download",
    yt_id: "audit0",
    status: "downloading",
    progress: 0.43,
  });
  await expect(page.getByText("43%", { exact: true })).toBeVisible();
  await expect(waveform).toHaveAttribute("data-pending", "false");
  await expect(rowButton(0)).toHaveAccessibleName("Downloading (43%)");

  send({
    type: "audio-download",
    yt_id: "audit0",
    status: "done",
    progress: null,
  });
  await expect(page.getByText("43%", { exact: true })).toHaveCount(0);
  await expect(page.getByText("--:--", { exact: true })).toBeVisible();
  await expect(rowButton(0)).toHaveAccessibleName("Downloaded");
});

test("an evicted track stays shown as not downloaded when its buffered audio plays on", async ({
  page,
}) => {
  const { send, messages } = await openApp(page, {
    audio: "ok",
    audioSeconds: 240,
  });
  await startPlayback(page);
  const button = page.locator(".play-pause-button");
  const rowButton = page.locator('.ag-row[row-index="0"] .audio-cache-button');
  await expect(button).toHaveAccessibleName("Pause");
  send({ type: "audio-download", yt_id: "audit0", status: "done" });
  await expect(rowButton).toHaveAccessibleName("Downloaded");
  await button.click();
  await expect(button).toHaveAccessibleName("Play");
  send({ type: "audio-evicted", yt_ids: ["audit0"] });
  await expect(rowButton).toHaveAccessibleName("Download");
  await button.click();
  await expect(button).toHaveAccessibleName("Pause");
  await expect(button).not.toHaveAttribute("aria-busy", "true");
  await expect(rowButton).toHaveAccessibleName("Download");
  await rowButton.click();
  await expect
    .poll(() => messages.filter((m) => m.type === "download-audio"))
    .toEqual([{ type: "download-audio", yt_id: "audit0" }]);
});

/** Pauses playback at `seconds` (in steps of five) into the track. */
async function pauseAt(page: Page, seconds: number) {
  const button = page.locator(".play-pause-button");
  const slider = page.getByRole("slider", {
    name: "Playback position",
    exact: true,
  });
  await expect(slider).toBeEnabled();
  await button.click();
  await expect(button).toHaveAccessibleName("Play");
  await slider.focus();
  await page.keyboard.press("Home");
  for (let i = 0; i < seconds / 5; i++) await page.keyboard.press("ArrowRight");
  await expect
    .poll(async () => Number(await slider.getAttribute("aria-valuenow")))
    .toBeCloseTo(seconds, 0);
  return slider;
}

test("a paused track's row play button resumes it where it was", async ({
  page,
}) => {
  await openApp(page, { audio: "ok", audioSeconds: 240 });
  await startPlayback(page);
  const button = page.locator(".play-pause-button");
  await expect(button).toHaveAccessibleName("Pause");
  const slider = await pauseAt(page, 30);
  await page.locator('.ag-row[row-index="0"] .thumbnail-play-button').click();
  await expect(button).toHaveAccessibleName("Pause");
  // Playing on from there, not from the start.
  await expect
    .poll(async () => Number(await slider.getAttribute("aria-valuenow")))
    .toBeGreaterThan(31);
});

test("a search result resumed from the collection plays on in the collection", async ({
  page,
}) => {
  const { audioRequests, holdQueue } = await openApp(page, {
    audio: "ok",
    audioSeconds: 15,
    searchResults: [
      { title: "Result A", artists: ["Artist A"], yt_id: "audit0", bp_id: 1 },
      searchResults[1],
    ],
  });
  await page.getByRole("textbox", { name: /Search query/ }).fill("results");
  await page.keyboard.press("Enter");
  await page
    .getByRole("button", { name: "Play Result A", exact: true })
    .click();
  const button = page.locator(".play-pause-button");
  await expect(button).toHaveAccessibleName("Pause");
  await pauseAt(page, 10);
  await page.getByRole("button", { name: "Exit search", exact: true }).click();
  // The collection's queue arrives only after the track has ended.
  holdQueue(true);
  // Played next, rather than prefetched (with HEAD) while Result A played.
  const before = audioRequests.length;
  const played = (id: string) =>
    audioRequests.slice(before).some((r) => r.startsWith(`GET /audio/${id}?`));
  await page.locator('.ag-row[row-index="0"] .thumbnail-play-button').click();
  await page.waitForTimeout(6500);
  expect(played("bVideo00002")).toBe(false);
  holdQueue(false);
  await expect.poll(() => played("audit1"), { timeout: 10_000 }).toBe(true);
  expect(played("bVideo00002")).toBe(false);
});

test("the background pulses with the playing track's beats", async ({
  page,
}) => {
  const { send, messages } = await openApp(page, {
    audio: "ok",
    audioSeconds: 240,
  });
  await startPlayback(page);
  const button = page.locator(".play-pause-button");
  await expect(button).toHaveAccessibleName("Pause");
  send({ type: "audio-download", yt_id: "audit0", status: "done" });
  await expect
    .poll(() => messages.some((m) => m.type === "get-waveform"))
    .toBe(true);
  const request = messages.find((m) => m.type === "get-waveform")!;
  send({
    type: "waveform",
    id: request.id,
    yt_id: "audit0",
    peaks: new Array(1000).fill(128),
    // Every half second, at full strength.
    beats: Array.from({ length: 480 }, (_, i) => [i * 500, 255]),
  });
  const background = page.locator(".background-player");
  /** The pulses over a beat and a half, as the background shows them. */
  const pulses = () =>
    background.evaluate(
      (element: HTMLElement) =>
        new Promise<number[]>((resolve) => {
          const values: number[] = [];
          const started = performance.now();
          requestAnimationFrame(function sample() {
            values.push(Number(element.style.getPropertyValue("--beat") || 0));
            if (performance.now() - started < 750)
              requestAnimationFrame(sample);
            else resolve(values);
          });
        }),
    );
  await expect
    .poll(async () => {
      const values = await pulses();
      return Math.max(...values) > 0.5 && Math.min(...values) < 0.2;
    })
    .toBe(true);

  await button.click();
  await expect(button).toHaveAccessibleName("Play");
  expect(Math.max(...(await pulses()))).toBe(0);

  await page.emulateMedia({ reducedMotion: "reduce" });
  await button.click();
  await expect(button).toHaveAccessibleName("Pause");
  expect(Math.max(...(await pulses()))).toBe(0);
});

/**
 * Plays the first track (130 BPM in its metadata) and replies to its waveform
 * request with `tempo`, after calling `before`.
 */
async function playWithTempo(
  page: Page,
  tempo: { bpm: number; steady: boolean },
  before?: (app: Awaited<ReturnType<typeof openApp>>) => void,
) {
  const app = await openApp(page, { audio: "ok", audioSeconds: 240 });
  await startPlayback(page);
  before?.(app);
  await expect(page.locator(".play-pause-button")).toHaveAccessibleName(
    "Pause",
  );
  app.send({ type: "audio-download", yt_id: "audit0", status: "done" });
  await expect
    .poll(() => app.messages.some((m) => m.type === "get-waveform"))
    .toBe(true);
  const request = app.messages.find((m) => m.type === "get-waveform")!;
  app.send({
    type: "waveform",
    id: request.id,
    yt_id: "audit0",
    peaks: new Array(1000).fill(128),
    beats: [],
    tempo,
  });
  return app;
}

test("offers to correct a BPM that deviates from the measured tempo", async ({
  page,
}) => {
  const { messages } = await playWithTempo(page, { bpm: 126.04, steady: true });
  await expect(page.locator(".now-playing-tempo")).toHaveText("126 BPM");
  await expect(
    toastText(page, "Example track 1 plays at 126 BPM, not 130 BPM"),
  ).toBeVisible();
  await page.getByRole("button", { name: "Set to 126" }).click();
  await expect
    .poll(() => messages.filter((m) => m.type === "update-bpm"))
    .toEqual([
      {
        type: "update-bpm",
        yt_id: "audit0",
        bpm: 126,
        collection: "/audit/collection.db",
      },
    ]);
  await expect(toastText(page, "plays at")).toHaveCount(0);
});

test("a BPM suggestion is withdrawn when the collection is switched", async ({
  page,
}) => {
  const app = await playWithTempo(page, { bpm: 126.04, steady: true });
  const toast = toastText(page, "plays at 126 BPM, not 130");
  await expect(toast).toBeVisible();
  app.send({
    type: "collection-info",
    collection: { path: "/audit/second.db", size: app.tracks.length },
  });
  await expect(toast).toHaveCount(0);
  expect(app.messages.filter((m) => m.type === "update-bpm")).toEqual([]);
  // Not dismissed: offered again back in the collection it was checked against.
  expect(
    await page.evaluate(() => localStorage.getItem("stereo-bpm-dismissed")),
  ).toBeNull();
  app.send({ type: "collection-info", collection: app.collection });
  await expect(toast).toBeVisible();
});

test("a BPM suggestion is based on the metadata stored in the collection", async ({
  page,
}) => {
  const { messages } = await playWithTempo(
    page,
    { bpm: 126.04, steady: true },
    // The playing track's metadata, e.g. a search result's, has half its stored BPM.
    (app) =>
      app.send({ type: "track-info", track: { ...app.tracks[0], bpm: 65 } }),
  );
  await expect(
    toastText(page, "Example track 1 plays at 126 BPM, not 130 BPM"),
  ).toBeVisible();
  await page.getByRole("button", { name: "Set to 126" }).click();
  await expect
    .poll(() => messages.filter((m) => m.type === "update-bpm"))
    .toMatchObject([{ yt_id: "audit0", bpm: 126 }]);
});

test("a track gets a BPM suggestion once it is added to the collection", async ({
  page,
}) => {
  const app = await playWithTempo(page, { bpm: 126.04, steady: true }, (app) =>
    app.absent.add("audit0"),
  );
  await expect
    .poll(() => app.messages.some((m) => m.type === "get-collection-track"))
    .toBe(true);
  const toast = toastText(page, "plays at 126 BPM, not 130");
  await page.waitForTimeout(500);
  await expect(toast).toHaveCount(0);
  app.absent.delete("audit0");
  app.send({ type: "collection-info", collection: app.collection });
  await expect(toast).toBeVisible();
});

test("a dismissed BPM suggestion isn't offered again", async ({ page }) => {
  await playWithTempo(page, { bpm: 124.4, steady: true });
  const toast = toastText(page, "plays at 124.4 BPM, not 130");
  await expect(toast).toBeVisible();
  await page.getByRole("button", { name: /dismiss|close/i }).click();
  await expect(toast).toHaveCount(0);
  expect(
    await page.evaluate(() => localStorage.getItem("stereo-bpm-dismissed")),
  ).toBe(JSON.stringify(["audit0:124"]));
});

test("half time isn't taken for wrong metadata", async ({ page }) => {
  await playWithTempo(page, { bpm: 65, steady: true });
  // Shown at the metadata's octave.
  await expect(page.locator(".now-playing-tempo")).toHaveText("130 BPM");
  await page.waitForTimeout(500);
  await expect(toastText(page, "plays at")).toHaveCount(0);
});

test("a varying tempo is shown as approximate and not suggested", async ({
  page,
}) => {
  await playWithTempo(page, { bpm: 126.3, steady: false });
  await expect(page.locator(".now-playing-tempo")).toHaveText("~126 BPM");
  await page.waitForTimeout(500);
  await expect(toastText(page, "plays at")).toHaveCount(0);
});

test("a track without a waveform gets one once its audio is downloaded again", async ({
  page,
}) => {
  const { send, messages } = await openApp(page);
  const requests = () => messages.filter((m) => m.type === "get-waveform");
  const reply = (peaks: number[] | null) => {
    const request = requests().at(-1)!;
    send({ type: "waveform", id: request.id, yt_id: "audit0", peaks });
  };
  // Bars of the whole waveform, which start at the top only at full loudness.
  const bars = page.locator(".waveform .waveform-unplayed");

  // E.g. evicted between the client learning it's downloaded and asking.
  send({ type: "audio-cached", yt_ids: ["audit0"] });
  await expect.poll(() => requests().length).toBe(1);
  reply(null);
  send({ type: "audio-evicted", yt_ids: ["audit0"] });
  send({ type: "audio-download", yt_id: "audit0", status: "done" });
  await expect.poll(() => requests().length).toBe(2);
  reply(new Array(1000).fill(255));
  await expect(bars).toHaveAttribute("d", /^M0 0\.0h/);
});

test("the play button shows that playback is loading and can cancel it", async ({
  page,
}) => {
  const { audioRequests } = await openApp(page);
  await startPlayback(page);
  await expect
    .poll(() => audioRequests)
    .toContain("GET /audio/audit0?key=test-key");
  const button = page.locator(".play-pause-button");
  await expect(button).toHaveAttribute("aria-busy", "true");
  await expect(button).toHaveAccessibleName("Pause");
  // Nothing to seek in before the duration is known.
  await expect(
    page.getByRole("slider", { name: "Playback position", exact: true }),
  ).toBeDisabled();
  await expect(page.getByText("--:--", { exact: true })).toBeVisible();
  await button.click();
  await expect(button).not.toHaveAttribute("aria-busy", "true");
  await expect(button).toHaveAccessibleName("Play");
});

test("media session offers track skipping rather than ±10s seeking", async ({
  page,
}) => {
  // iOS shows either seek or track buttons on the lock screen. It ignores
  // handlers registered before the audio is its "Now Playing" item and then
  // falls back to seek buttons, so they're registered again once a track plays.
  await page.addInitScript(() => {
    const handlers = new Map<string, unknown>();
    const calls: string[] = [];
    Reflect.set(window, "__mediaSessionHandlers", handlers);
    Reflect.set(window, "__mediaSessionCalls", calls);
    const set = navigator.mediaSession.setActionHandler.bind(
      navigator.mediaSession,
    );
    navigator.mediaSession.setActionHandler = (action, handler) => {
      calls.push(`${action} ${handler ? "set" : "cleared"}`);
      if (handler) handlers.set(action, handler);
      else handlers.delete(action);
      set(action, handler);
    };
    // Marks when the app's audio element starts playing (listening before the app does).
    const NativeAudio = window.Audio;
    window.Audio = function (src?: string) {
      const audio = new NativeAudio(src);
      audio.addEventListener("playing", () => calls.push("playing"));
      return audio;
    } as unknown as typeof Audio;
  });
  await openApp(page, { audio: "ok", audioSeconds: 240 });
  await startPlayback(page);
  const calls = () =>
    page.evaluate((): string[] => Reflect.get(window, "__mediaSessionCalls"));
  await expect.poll(calls).toContain("playing");
  const actions = () =>
    page.evaluate(() =>
      [...Reflect.get(window, "__mediaSessionHandlers").keys()].sort(),
    );
  expect(await actions()).toEqual([
    "nexttrack",
    "pause",
    "play",
    "previoustrack",
    "seekto",
  ]);
  const all = await calls();
  const sincePlaying = all.slice(all.lastIndexOf("playing") + 1);
  expect(sincePlaying).toEqual(
    expect.arrayContaining([
      "nexttrack set",
      "previoustrack set",
      "seekbackward cleared",
      "seekforward cleared",
    ]),
  );
});

test("quick playlist edits each change only their own field", async ({
  page,
}) => {
  const { messages, send } = await openApp(page);
  send({
    type: "playlists",
    playlists: [
      {
        id: "p1",
        name: "Warmup",
        collection: "/audit/collection.db",
        filter_model: {},
        sort_model: [],
        limit: null,
        spotify: null,
        youtube: null,
        updated_at: 1,
        spotify_status: null,
        youtube_status: null,
      },
    ],
  });
  await page.getByRole("button", { name: "Playlists", exact: true }).click();
  await page.getByRole("menuitem", { name: /Manage playlists/ }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("Name").fill("Peak time");
  // The mock never answers, as if the rename's reply hadn't arrived yet.
  await dialog.getByRole("switch", { name: "Export to Spotify" }).click();
  await expect
    .poll(() => messages.filter((m) => m.type === "edit-playlist"))
    .toEqual([
      {
        type: "edit-playlist",
        playlist_id: "p1",
        edit: { kind: "rename", name: "Peak time" },
      },
      {
        type: "edit-playlist",
        playlist_id: "p1",
        edit: {
          kind: "set-export",
          service: "spotify",
          target: { auto_sync: true },
        },
      },
    ]);
  expect(messages.some((m) => m.type === "save-playlist")).toBe(false);
});

const pairingCode = "0123456789abcdef0123456789abcdef";

/** Records the subprotocols each `/ws` connection offers (call after `routeWebSocket`). */
async function recordWsProtocols(page: Page) {
  await page.addInitScript(() => {
    const offered: (string | string[] | null)[] = [];
    Object.assign(window, { offeredWsProtocols: offered });
    window.WebSocket = class extends window.WebSocket {
      constructor(url: string | URL, protocols?: string | string[]) {
        super(url, protocols);
        if (new URL(url).pathname === "/ws") offered.push(protocols ?? null);
      }
    };
  });
  return () =>
    page.evaluate(
      () =>
        (window as unknown as { offeredWsProtocols: unknown })
          .offeredWsProtocols,
    );
}

test("a pairing code survives a failed connection and is traded for the token", async ({
  page,
}) => {
  const sockets: WebSocketRoute[] = [];
  await page.routeWebSocket("**/ws", (ws) => {
    sockets.push(ws);
    // The first attempt fails as an unreachable backend would, before redeeming the code.
    if (sockets.length === 1) return void ws.close({ code: 1011 });
    ws.send(
      JSON.stringify([
        { type: "backend-info", version: "test", home_dir: "/audit" },
        { type: "auth-token", token: "tailnet-token" },
        { type: "repos", repos: [] },
      ]),
    );
  });
  const offered = await recordWsProtocols(page);
  await page.goto(`/#stereo-pair=${pairingCode}`);
  const pair = [`stereo.pair.${pairingCode}`];
  await expect.poll(offered, { timeout: 10_000 }).toEqual([pair, pair]);
  await expect
    .poll(() => page.evaluate(() => localStorage.getItem("stereo-ws-token")))
    .toBe("tailnet-token");
  // Paired: later connections offer the token instead.
  await sockets[1].close();
  const auth = `stereo.auth.${Buffer.from("tailnet-token").toString("hex")}`;
  await expect.poll(offered, { timeout: 10_000 }).toEqual([pair, pair, [auth]]);
  await expect(toastText(page, /Sign-in code expired/)).toHaveCount(0);
});

test("a rejected pairing code is explained and not offered again", async ({
  page,
}) => {
  await page.routeWebSocket("**/ws", (ws) => {
    ws.close({ code: 4401, reason: "Invalid or expired pairing code" });
  });
  const offered = await recordWsProtocols(page);
  await page.goto(`/#stereo-pair=${pairingCode}`);
  await expect(toastText(page, /Sign-in code expired/)).toBeVisible();
  // Reconnects keep going, without the code.
  await expect
    .poll(async () => ((await offered()) as unknown[]).slice(0, 2), {
      timeout: 10_000,
    })
    .toEqual([[`stereo.pair.${pairingCode}`], null]);
});

test("a rejected saved token is forgotten and explained", async ({ page }) => {
  await page.addInitScript(() => {
    if (!sessionStorage.getItem("seeded")) {
      sessionStorage.setItem("seeded", "1");
      localStorage.setItem("stereo-ws-token", "old-token");
    }
  });
  await page.routeWebSocket("**/ws", (ws) => {
    ws.close({ code: 4403, reason: "Invalid token" });
  });
  const offered = await recordWsProtocols(page);
  await page.goto("/");
  await expect(toastText(page, /Saved sign-in no longer valid/)).toBeVisible();
  expect(
    await page.evaluate(() => localStorage.getItem("stereo-ws-token")),
  ).toBeNull();
  // Reconnects keep going, without the token.
  const auth = `stereo.auth.${Buffer.from("old-token").toString("hex")}`;
  await expect
    .poll(async () => ((await offered()) as unknown[]).slice(0, 2), {
      timeout: 10_000,
    })
    .toEqual([[auth], null]);
});

const searchResults = [
  { title: "Result A", artists: ["Artist A"], yt_id: "aVideo00001", bp_id: 1 },
  { title: "Result B", artists: ["Artist B"], yt_id: "bVideo00002", bp_id: 2 },
  { title: "Result C", artists: ["Artist C"], yt_id: "cVideo00003", bp_id: 3 },
  { title: "Result D", artists: ["Artist D"], yt_id: "dVideo00004", bp_id: 4 },
];

/** Searches, and waits for the results above. */
async function showSearchResults(page: Page) {
  await page.getByRole("textbox", { name: /Search query/ }).fill("results");
  await page.keyboard.press("Enter");
  await expect(
    page.getByRole("button", { name: "Play Result D", exact: true }),
  ).toBeVisible();
}

/** Changes a search result's video to a pasted link's. */
async function changeResultVideo(page: Page, title: string, yt_id: string) {
  await page
    .getByRole("button", { name: `Options for ${title}`, exact: true })
    .click();
  await page
    .getByRole("menuitem", { name: "Change YouTube video…", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "Change YouTube video" });
  await dialog
    .getByRole("searchbox", { name: "Search YouTube or paste a video link" })
    .fill(`https://www.youtube.com/watch?v=${yt_id}`);
  await page.keyboard.press("Enter");
  const use = dialog.getByRole("button", { name: "Use this video" });
  await expect(use).toBeEnabled();
  await use.click();
  await expect(dialog).toHaveCount(0);
}

test("a search result warns of similar tracks in the collection", async ({
  page,
}) => {
  const { messages } = await openApp(page, {
    searchResults,
    similarTracks: {
      bVideo00002: [
        {
          yt_id: "otherVideo1",
          title: "Result B",
          mix_name: "Extended Mix",
          artists: ["Artist B"],
          bpm: 124,
          key: "8A",
        },
      ],
    },
  });
  await showSearchResults(page);
  await expect
    .poll(
      () => messages.find((m) => m.type === "find-similar-tracks")?.tracks,
    )
    .toContainEqual(
    expect.objectContaining({
      yt_id: "bVideo00002",
      title: "Result B",
      artists: ["Artist B"],
      bp_id: 2,
    }),
  );
  const label = "A similar track is in the collection";
  const warnings = page.getByRole("button", { name: label, exact: true });
  await expect(warnings).toHaveCount(1);
  await expect(
    page
      .locator(".ag-row", { has: warnings })
      .getByRole("button", { name: "Play Result B", exact: true }),
  ).toBeVisible();
  await warnings.hover();
  const card = page.getByRole("dialog", { name: label });
  await expect(card).toContainText("Artist B – Result B");
  await expect(card).toContainText("Extended Mix · 124 BPM · 8A");
  // Adding stays possible.
  await expect(
    page
      .locator(".ag-row", { has: warnings })
      .getByRole("button", { name: "Add to collection", exact: true }),
  ).toBeVisible();
});

test("changing a search result's video leaves results sharing its old video alone", async ({
  page,
}) => {
  const { audioRequests } = await openApp(page, {
    audio: "ok",
    searchResults,
  });
  await showSearchResults(page);
  await changeResultVideo(page, "Result A", "bVideo00002");
  await changeResultVideo(page, "Result B", "newVideo001");
  await page
    .getByRole("button", { name: "Play Result A", exact: true })
    .click();
  await expect
    .poll(() => audioRequests)
    .toContain("GET /audio/bVideo00002?key=test-key");
  await page
    .getByRole("button", { name: "Play Result B", exact: true })
    .click();
  await expect
    .poll(() => audioRequests)
    .toContain("GET /audio/newVideo001?key=test-key");
});

test("a queue of search results plays a result's changed video", async ({
  page,
}) => {
  const { audioRequests } = await openApp(page, {
    audio: "hold",
    searchResults,
  });
  await showSearchResults(page);
  await page
    .getByRole("button", { name: "Play Result A", exact: true })
    .click();
  await expect
    .poll(() => audioRequests)
    .toContain("GET /audio/aVideo00001?key=test-key");
  await changeResultVideo(page, "Result B", "newVideo001");
  await page
    .getByRole("button", { name: "Skip to next track", exact: true })
    .click();
  await expect
    .poll(() => audioRequests)
    .toContain("GET /audio/newVideo001?key=test-key");
  expect(audioRequests).not.toContain("GET /audio/bVideo00002?key=test-key");
});

test("changing the playing search result's video keeps its place in the queue", async ({
  page,
}) => {
  const { audioRequests } = await openApp(page, {
    audio: "ok",
    audioSeconds: 30,
    searchResults,
  });
  await showSearchResults(page);
  await page
    .getByRole("button", { name: "Play Result A", exact: true })
    .click();
  await expect(page.locator(".play-pause-button")).toHaveAccessibleName(
    "Pause",
  );
  await changeResultVideo(page, "Result A", "newVideo001");
  await expect
    .poll(() => audioRequests)
    .toContain("GET /audio/newVideo001?key=test-key");
  await page
    .getByRole("button", { name: "Skip to next track", exact: true })
    .click();
  await expect
    .poll(() => audioRequests)
    .toContain("GET /audio/bVideo00002?key=test-key");
});

test("the header and toolbar's frosted glass leaves solid buttons alone", async ({
  page,
}) => {
  await openApp(page, { searchResults });
  await showSearchResults(page);
  const glass = (name: string | RegExp) =>
    page
      .getByRole("button", { name, exact: typeof name === "string" })
      .evaluate((el) => getComputedStyle(el).backdropFilter);
  expect(await glass("Settings")).toBe("blur(8px)");
  expect(await glass(/^Add selected/)).toBe("none");
});

test("changing a selected search result's video keeps it selected", async ({
  page,
}) => {
  const { messages } = await openApp(page, { searchResults });
  await showSearchResults(page);
  const rowA = page.locator(".ag-row", {
    has: page.getByRole("button", { name: "Play Result A", exact: true }),
  });
  await rowA.getByRole("checkbox").first().check();
  await expect(
    page.getByRole("button", { name: "Add selected (1)", exact: true }),
  ).toBeEnabled();
  await changeResultVideo(page, "Result A", "newVideo001");
  // Past the grid's row animations, which would drop a replaced row.
  await page.waitForTimeout(600);
  await expect(rowA.getByRole("checkbox").first()).toBeChecked();
  const add = page.getByRole("button", {
    name: "Add selected (1)",
    exact: true,
  });
  await expect(add).toBeEnabled();
  await add.click();
  await expect
    .poll(() =>
      messages
        .filter((m) => m.type === "add-tracks")
        .flatMap((m) => (m.tracks as { yt_id: string }[]).map((t) => t.yt_id)),
    )
    .toEqual(["newVideo001"]);
});

test("a queue of search results sharing a video plays through to its end", async ({
  page,
}) => {
  const { audioRequests } = await openApp(page, {
    audio: "hold",
    searchResults,
  });
  await showSearchResults(page);
  // The queue's videos: a, b, a, d.
  await changeResultVideo(page, "Result C", "aVideo00001");
  await page
    .getByRole("button", { name: "Play Result A", exact: true })
    .click();
  const next = page.getByRole("button", {
    name: "Skip to next track",
    exact: true,
  });
  const playing = page.locator(".grid .progress-bar");
  for (const title of ["Result B", "Result C", "Result D"]) {
    await next.click();
    await expect(playing).toHaveCount(1);
    await expect(playing).toContainText(title);
  }
  await expect
    .poll(() => audioRequests)
    .toContain("GET /audio/dVideo00004?key=test-key");
  await page
    .getByRole("button", { name: "Skip to previous track", exact: true })
    .click();
  await expect(playing).toContainText("Result C");
});

test("changing a result's video leaves another result playing the same video alone", async ({
  page,
}) => {
  const { audioRequests } = await openApp(page, {
    audio: "hold",
    searchResults,
  });
  await showSearchResults(page);
  await changeResultVideo(page, "Result C", "aVideo00001");
  // Result C plays, from a queue whose first entry, Result A, has the same video.
  await page
    .getByRole("button", { name: "Play Result A", exact: true })
    .click();
  const next = page.getByRole("button", {
    name: "Skip to next track",
    exact: true,
  });
  await next.click();
  await next.click();
  const playing = page.locator(".grid .progress-bar");
  await expect(playing).toHaveCount(1);
  await expect(playing).toContainText("Result C");
  await changeResultVideo(page, "Result A", "newVideo001");
  await expect(playing).toHaveCount(1);
  await expect(playing).toContainText("Result C");
  expect(audioRequests).not.toContain("GET /audio/newVideo001?key=test-key");
});

test("search sources are alphabetical and reset to the defaults", async ({
  page,
}) => {
  await openApp(page);
  await page.getByRole("button", { name: "Search options" }).click();
  const sources = page.getByRole("menuitemcheckbox");
  await expect(sources).toHaveText([
    "Apple Music",
    "Beatport",
    "SoundCloud",
    "Spotify",
    "Traxsource",
    "Volumo",
  ]);
  const reset = page.getByRole("menuitem", { name: "Reset to defaults" });
  await expect(reset).toHaveAttribute("aria-disabled", "true");

  await page.getByRole("menuitemcheckbox", { name: "Beatport" }).click();
  await page.getByRole("menuitemcheckbox", { name: "Spotify" }).click();
  await expect(reset).not.toHaveAttribute("aria-disabled", "true");
  await reset.click();
  await expect(reset).toHaveAttribute("aria-disabled", "true");
  for (const [name, checked] of [
    ["Apple Music", false],
    ["Beatport", true],
    ["SoundCloud", false],
    ["Spotify", false],
    ["Traxsource", true],
    ["Volumo", true],
  ] as const) {
    await expect(page.getByRole("menuitemcheckbox", { name })).toHaveAttribute(
      "aria-checked",
      `${checked}`,
    );
  }
});

test("search options persist across reloads", async ({ page }) => {
  await openApp(page);
  const options = page.getByRole("button", { name: "Search options" });
  await options.click();
  await page.getByRole("menuitemradio", { name: "25", exact: true }).click();
  await page.getByRole("menuitemcheckbox", { name: "Volumo" }).click();
  await page.getByRole("menuitemcheckbox", { name: "Spotify" }).click();
  await page.keyboard.press("Escape");

  await page.reload();
  await options.click();
  await expect(
    page.getByRole("menuitemradio", { name: "25", exact: true }),
  ).toHaveAttribute("aria-checked", "true");
  for (const [name, checked] of [
    ["Beatport", true],
    ["Spotify", true],
    ["Traxsource", true],
    ["Volumo", false],
  ] as const) {
    await expect(page.getByRole("menuitemcheckbox", { name })).toHaveAttribute(
      "aria-checked",
      `${checked}`,
    );
  }
});

test("invalid stored search options fall back to the defaults", async ({
  page,
}) => {
  await page.addInitScript(() => {
    localStorage.setItem("stereo_search_limit", "7");
    localStorage.setItem("stereo_search_sources", '["napster","shazam"]');
  });
  await openApp(page);
  await page.getByRole("button", { name: "Search options" }).click();
  await expect(
    page.getByRole("menuitemradio", { name: "100", exact: true }),
  ).toHaveAttribute("aria-checked", "true");
  await expect(
    page.getByRole("menuitem", { name: "Reset to defaults" }),
  ).toHaveAttribute("aria-disabled", "true");
});

test("malformed stored search options are replaced on the next change", async ({
  page,
}) => {
  await page.addInitScript(() => {
    if (sessionStorage.getItem("seeded")) return;
    sessionStorage.setItem("seeded", "1");
    localStorage.setItem("stereo_search_limit", "malformed");
    localStorage.setItem("stereo_search_sources", "[");
  });
  await openApp(page);
  const options = page.getByRole("button", { name: "Search options" });
  await options.click();
  const limit = page.getByRole("menuitemradio", { name: "25", exact: true });
  const spotify = page.getByRole("menuitemcheckbox", { name: "Spotify" });
  await expect(
    page.getByRole("menuitem", { name: "Reset to defaults" }),
  ).toHaveAttribute("aria-disabled", "true");
  await limit.click();
  await spotify.click();
  await page.keyboard.press("Escape");

  await page.reload();
  await options.click();
  await expect(limit).toHaveAttribute("aria-checked", "true");
  await expect(spotify).toHaveAttribute("aria-checked", "true");
});

test("label searches explain that Spotify can't search by label", async ({
  page,
}) => {
  await openApp(page);
  const options = page.getByRole("button", { name: "Search options" });
  await options.click();
  for (const source of ["Beatport", "Volumo", "Traxsource", "Spotify"]) {
    await page.getByRole("menuitemcheckbox", { name: source }).click();
  }
  await page.keyboard.press("Escape");
  await page.getByRole("textbox", { name: /Search query/ }).fill("Defected");
  const search = page.getByRole("button", { name: "Search", exact: true });
  await expect(search).toBeEnabled();

  await page.getByRole("combobox", { name: "Search kind" }).click();
  await page.getByRole("option", { name: "Label", exact: true }).click();
  await expect(
    page.getByText(/Spotify and Apple Music can't search by label/).first(),
  ).toBeVisible();
  await expect(search).toBeDisabled();
  await options.click();
  const spotify = page.getByRole("menuitemcheckbox", { name: /Spotify/ });
  await expect(spotify).toHaveAttribute("aria-disabled", "true");
  await expect(spotify).toContainText("Can't search by label");
});

/** Opens the collection menu, downloads the open collection and answers with the
 * token `fixture-server.ts` serves. */
async function prepareDownload(
  page: Page,
  { messages, send }: Awaited<ReturnType<typeof openApp>>,
) {
  await page
    .locator(".collection-controls")
    .getByRole("button")
    .first()
    .click();
  await page.getByRole("menuitem", { name: "Download collection.db" }).click();
  const requests = () =>
    messages.filter(
      (message) => message.type === "prepare-collection-download",
    );
  await expect
    .poll(requests)
    .toEqual([expect.objectContaining({ collection: "/audit/collection.db" })]);
  send({ type: "collection-download", id: requests()[0].id, token: "fixture" });
}

test("the collection menu downloads the collection file @webkit", async ({
  page,
}) => {
  const app = await openApp(page);
  const download = page.waitForEvent("download");
  await prepareDownload(page, app);
  const file = await download;
  expect(await file.failure()).toBeNull();
  expect(file.suggestedFilename().normalize()).toBe("Tänze.db");
  const path = await file.path();
  expect(readFileSync(path, "utf8")).toBe(collectionFixture);
});

test("the desktop app offers a collection download as a link @webkit", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as { __STEREO_DESKTOP__?: boolean }).__STEREO_DESKTOP__ = true;
  });
  const app = await openApp(page);
  await prepareDownload(page, app);
  const dialog = page.getByRole("dialog", { name: "Download Collection" });
  // Opened by the click, not from script once the reply arrives, so WebKitGTK
  // doesn't block it as a popup; the app opens it in the browser.
  const link = dialog.getByRole("link", { name: "Download" });
  await expect(link).toHaveAttribute("href", "/collection-download/fixture");
  await expect(link).toHaveAttribute("target", "_blank");
  const request = page
    .context()
    .waitForEvent("request", (r) =>
      r.url().endsWith("/collection-download/fixture"),
    );
  const popup = page.waitForEvent("popup");
  await link.click();
  await popup;
  await request;
  await expect(dialog).toHaveCount(0);
});

/** Opens the collection menu's import dialog and picks `Warm-up sets.db`. */
async function pickCollectionFile(page: Page) {
  await page
    .locator(".collection-controls")
    .getByRole("button")
    .first()
    .click();
  await page.getByRole("menuitem", { name: "Import collection file…" }).click();
  const dialog = page.getByRole("dialog", { name: "Import Collection" });
  await expect(dialog.getByRole("button", { name: "Import" })).toBeDisabled();
  await dialog.locator('input[type="file"]').setInputFiles({
    name: "Warm-up sets.db",
    mimeType: "application/octet-stream",
    buffer: Buffer.from(importedFile),
  });
  return dialog;
}

const importedFile = "SQLite format 3\0";

test("the collection menu imports a collection file as is @webkit", async ({
  page,
}) => {
  const { messages, send } = await openApp(page);
  const imports = () => messages.filter((m) => m.type === "import-collection");
  const dialog = await pickCollectionFile(page);
  const name = dialog.getByRole("textbox", { name: "Collection name" });
  await expect(name).toHaveValue("Warm-up sets");
  await name.fill("warmup");

  // The Enter that commits an input method editor's text doesn't import.
  await name.dispatchEvent("keydown", composingEnter);
  await page.waitForTimeout(200);
  expect(imports()).toEqual([]);

  // Into the repository selected, and the dialog waits for the backend.
  await name.press("Enter");
  await expect.poll(imports).toEqual([
    {
      type: "import-collection",
      id: expect.any(Number),
      repo: "/audit",
      name: "warmup",
      data: Buffer.from(importedFile).toString("base64"),
    },
  ]);
  const first = imports()[0] as { id: number };

  // A refusal keeps the file and name, saying why.
  send({ type: "error-reply", id: first.id, message: "Not a collection" });
  // Shown by the file input and announced by the dialog's live region.
  await expect(dialog.getByText("Not a collection").first()).toBeVisible();
  await expect(name).toHaveValue("warmup");

  await dialog.getByRole("button", { name: "Import" }).click();
  await expect.poll(() => imports().length).toBe(2);
  await expect(dialog).toBeVisible();
  const second = imports()[1] as { id: number };
  send({
    type: "collection-imported",
    id: second.id,
    path: "/audit/warmup.db",
  });
  await expect(dialog).toHaveCount(0);
});

test("importing a collection leaves the open playlist", async ({ page }) => {
  const { messages, send } = await openApp(page);
  send({
    type: "playlists",
    playlists: [
      {
        id: "p1",
        name: "Warmup",
        collection: "/audit/collection.db",
        filter_model: {},
        sort_model: [],
        limit: null,
        spotify: null,
        youtube: null,
        updated_at: 1,
        spotify_status: null,
        youtube_status: null,
      },
    ],
  });
  await page.getByRole("button", { name: "Playlists", exact: true }).click();
  await page.getByRole("menuitemradio", { name: /Warmup/ }).click();
  await expect(
    page.getByRole("button", { name: "Warmup", exact: true }),
  ).toBeVisible();

  const dialog = await pickCollectionFile(page);
  await dialog.getByRole("button", { name: "Import" }).click();
  const imports = () => messages.filter((m) => m.type === "import-collection");
  await expect.poll(() => imports().length).toBe(1);
  const { id } = imports()[0] as { id: number };
  send({ type: "collection-imported", id, path: "/audit/Warm-up sets.db" });
  await expect(dialog).toHaveCount(0);
  // The backend has opened the imported collection, which the playlist doesn't show.
  await expect(
    page.getByRole("button", { name: "Playlists", exact: true }),
  ).toBeVisible();
});

test("cancelling while the file is read doesn't import it @webkit", async ({
  page,
}) => {
  // Holds every file read until released.
  await page.addInitScript(() => {
    const read = FileReader.prototype.readAsDataURL;
    const w = window as { releaseRead?: () => void };
    FileReader.prototype.readAsDataURL = function (blob) {
      w.releaseRead = () => read.call(this, blob);
    };
  });
  const { messages } = await openApp(page);
  const dialog = await pickCollectionFile(page);
  await dialog.getByRole("button", { name: "Import" }).click();
  await expect
    .poll(() => page.evaluate(() => "releaseRead" in window))
    .toBe(true);
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toHaveCount(0);
  await page.evaluate(() =>
    (window as { releaseRead?: () => void }).releaseRead?.(),
  );
  await page.waitForTimeout(200);
  expect(messages.filter((m) => m.type === "import-collection")).toEqual([]);
});

test("the collection widget opens the open collection's stats", async ({
  page,
}) => {
  const { send } = await openApp(page);
  const stats = page.locator(".collection-group .collection-stats");
  await expect(stats).toBeDisabled();
  send({
    type: "collection-stats",
    top_artists_five_star: [
      { name: "Example artist", count: 3, weighted_avg_days_since_release: 10 },
    ],
    top_artists_four_plus: [],
    top_labels_five_star: [],
    top_labels_four_plus: [],
  });
  await stats.click();
  await expect(
    page.getByRole("dialog").getByText("Collection stats", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("dialog").getByText("Example artist", { exact: true }),
  ).toBeVisible();
  await page.keyboard.press("Escape");

  // Another collection opens: its stats come after, and until then (or if they never
  // do) there are none to show, not the previous collection's.
  send({
    type: "collection-info",
    collection: { path: "/tmp/other.db", size: 42 },
  });
  await expect(stats).toBeDisabled();
  send({
    type: "collection-stats",
    top_artists_five_star: [
      { name: "Other artist", count: 2, weighted_avg_days_since_release: 5 },
    ],
    top_artists_four_plus: [],
    top_labels_five_star: [],
    top_labels_four_plus: [],
  });
  await stats.click();
  await expect(
    page.getByRole("dialog").getByText("Other artist", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("dialog").getByText("Example artist", { exact: true }),
  ).toHaveCount(0);
});

test("the current view exports to a JSON file, optionally with user data", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as { __STEREO_DESKTOP__?: boolean }).__STEREO_DESKTOP__ = true;
  });
  const { messages, send } = await openApp(page);
  const exports = () =>
    messages.filter((m) => m.type === "prepare-tracks-download");

  await page.getByRole("button", { name: "Export tracks" }).click();
  await page.getByRole("menuitem", { name: "JSON file" }).last().click();
  const dialog = page.getByRole("dialog", { name: "Export to JSON" });
  await expect(dialog.getByText(/Saves \d+ tracks? as/)).toBeVisible();
  const name = dialog.getByRole("textbox", { name: "File name" });
  await expect(name).toHaveValue("collection");
  await name.fill("for a friend");
  await dialog
    .getByRole("checkbox", { name: "Include ratings and play history" })
    .check();
  await dialog.getByRole("button", { name: "Export" }).click();
  await expect(dialog).toHaveCount(0);

  await expect.poll(() => exports().length).toBe(1);
  const request = exports()[0] as {
    id: number;
    name: string;
    tracks: { yt_id: string }[];
    include_user_data: boolean;
  };
  expect(request.name).toBe("for a friend");
  expect(request.include_user_data).toBe(true);
  expect(request.tracks.length).toBeGreaterThan(0);

  send({ type: "collection-download", id: request.id, token: "fixture" });
  const download = page.getByRole("dialog", { name: "Download Tracks" });
  await expect(download.getByText("for a friend.json")).toBeVisible();
  await expect(
    download.getByRole("link", { name: "Download" }),
  ).toHaveAttribute("href", "/collection-download/fixture");
});

test("a JSON file of tracks imports into the open collection", async ({
  page,
}) => {
  const { messages, send } = await openApp(page);
  const imports = () => messages.filter((m) => m.type === "import-tracks");
  const file = JSON.stringify({
    format: "stereo-tracks",
    version: 1,
    tracks: [],
  });

  await page.getByRole("button", { name: "Import tracks" }).click();
  await page.getByRole("menuitem", { name: "JSON file" }).click();
  const dialog = page.getByRole("dialog", { name: "Import from JSON" });
  const importButton = dialog.getByRole("button", { name: "Import" });
  await expect(importButton).toBeDisabled();
  await dialog.locator('input[type="file"]').setInputFiles({
    name: "tracks.json",
    mimeType: "application/json",
    buffer: Buffer.from(file),
  });
  await importButton.click();
  await expect.poll(imports).toEqual([
    {
      type: "import-tracks",
      id: expect.any(Number),
      collection: "/audit/collection.db",
      // Parsed, so that it isn't escaped into a string.
      data: JSON.parse(file),
      keep_user_data: false,
    },
  ]);

  // A refusal keeps the file and choices, saying why.
  const first = imports()[0] as { id: number };
  const keep = dialog.getByRole("checkbox", {
    name: "Keep ratings and play history",
  });
  send({ type: "error-reply", id: first.id, message: "Not a tracks file" });
  await expect(dialog.getByText("Not a tracks file").first()).toBeVisible();
  await keep.check();
  await importButton.click();
  await expect.poll(() => imports().length).toBe(2);
  const second = imports()[1] as { id: number; keep_user_data: boolean };
  expect(second.keep_user_data).toBe(true);
  send({ type: "tracks-imported", id: second.id, imported: 3, skipped: 1 });
  await expect(dialog).toHaveCount(0);
  await expect(
    page
      .getByRole("region", { name: "Notifications" })
      .getByText("Imported 3 tracks, skipped 1 track already in collection"),
  ).toBeVisible();

  // Each import opts into someone else's ratings afresh.
  await page.getByRole("button", { name: "Import tracks" }).click();
  await page.getByRole("menuitem", { name: "JSON file" }).click();
  await expect(keep).not.toBeChecked();
  await keep.check();
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await page.getByRole("button", { name: "Import tracks" }).click();
  await page.getByRole("menuitem", { name: "JSON file" }).click();
  await expect(keep).not.toBeChecked();

  // A file that isn't JSON is refused before it is sent.
  await dialog.locator('input[type="file"]').setInputFiles({
    name: "tracks.json",
    mimeType: "application/json",
    buffer: Buffer.from("not json"),
  });
  await importButton.click();
  await expect(dialog.getByText(/isn't a JSON file/).first()).toBeVisible();
  expect(imports().length).toBe(2);
});

test("exporting an empty view to JSON says there's nothing to export", async ({
  page,
}) => {
  // A view filtered down to nothing.
  await page.addInitScript(() =>
    localStorage.setItem(
      "stereo_app_grid_state_v2",
      JSON.stringify({
        filter: {
          filterModel: {
            title: { filterType: "text", type: "contains", filter: "none" },
          },
        },
      }),
    ),
  );
  const { messages } = await openApp(page);
  await expect(
    page.getByRole("heading", { name: "No tracks match these filters" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Export tracks" }).click();
  await page.getByRole("menuitem", { name: "JSON file" }).last().click();
  await expect(
    page
      .getByRole("region", { name: "Notifications" })
      .getByText("No tracks in this view to export"),
  ).toBeVisible();
  await expect(
    page.getByRole("dialog", { name: "Export to JSON" }),
  ).toHaveCount(0);
  expect(messages.filter((m) => m.type === "prepare-tracks-download")).toEqual(
    [],
  );
});

const updateRelease = {
  version: "0.2.0",
  url: "https://github.com/buntec/stereo/releases/tag/v0.2.0",
  published_at: "2026-11-22T16:18:52Z",
};

test("the version shows whether a newer release is out", async ({ page }) => {
  const { send } = await openApp(page);
  await expect(page.getByText("vtest", { exact: true })).toBeVisible();
  // Nothing until the backend has checked.
  await expect(page.getByRole("button", { name: /^Stereo / })).toHaveCount(0);

  send({
    type: "update-check",
    status: {
      state: "available",
      latest: updateRelease,
      checked_at: new Date(Date.now() - 3 * 3600_000).toISOString(),
    },
  });
  const available = page.getByRole("button", {
    name: "Stereo v0.2.0 is available",
  });
  await available.hover();
  const card = page.getByRole("dialog", { name: "Stereo v0.2.0 is available" });
  await expect(card).toContainText("You're running vtest.");
  await expect(card).toContainText("Checked 3 h ago");
  await expect(
    card.getByRole("link", { name: /What's new in v0\.2\.0/ }),
  ).toHaveAttribute("href", updateRelease.url);

  await page.mouse.move(0, 400);
  await expect(card).toBeHidden();
  send({
    type: "update-check",
    status: {
      state: "up-to-date",
      latest: { ...updateRelease, url: "javascript:alert(1)" },
      checked_at: new Date(Date.now() - 2 * 86400_000).toISOString(),
    },
  });
  await page.getByRole("button", { name: "Stereo is up to date" }).hover();
  const upToDate = page.getByRole("dialog", { name: "Stereo is up to date" });
  await expect(upToDate).toContainText("Checked on ");
  // The backend decides whether versions are equal; the card just names them.
  await expect(upToDate).toContainText(
    "You're running vtest.The latest release is v0.2.0.",
  );
  // Only release pages are linked.
  await expect(
    upToDate.getByRole("link", { name: /Release notes for v0\.2\.0/ }),
  ).toHaveAttribute("href", "https://github.com/buntec/stereo/releases");

  // A failed refresh leaves a recent result standing, but mentions it.
  await page.mouse.move(0, 400);
  await expect(upToDate).toBeHidden();
  send({
    type: "update-check",
    status: {
      state: "up-to-date",
      latest: updateRelease,
      checked_at: new Date(Date.now() - 86400_000).toISOString(),
      refresh_error: {
        message: "network is unreachable",
        failed_at: new Date().toISOString(),
        is_stale: false,
      },
    },
  });
  await page.getByRole("button", { name: "Stereo is up to date" }).hover();
  await expect(upToDate).toContainText(
    "Checking again failed just now: network is unreachable",
  );

  // Once the result is old, the failure turns the icon into a warning.
  await page.mouse.move(0, 400);
  await expect(upToDate).toBeHidden();
  send({
    type: "update-check",
    status: {
      state: "up-to-date",
      latest: updateRelease,
      checked_at: new Date(Date.now() - 5 * 86400_000).toISOString(),
      refresh_error: {
        message: "connection refused",
        failed_at: new Date(Date.now() - 20 * 60_000).toISOString(),
        is_stale: true,
      },
    },
  });
  await page
    .getByRole("button", { name: "Couldn't check for updates" })
    .hover();
  const stale = page.getByRole("dialog", {
    name: "Couldn't check for updates",
  });
  await expect(stale).toContainText(
    "The latest release was v0.2.0 when checked.",
  );
  await expect(stale).toContainText("Checked on ");
  await expect(stale).toContainText(
    "Checking again failed 20 min ago: connection refused",
  );
  await expect(
    stale.getByRole("link", { name: /Release notes for v0\.2\.0/ }),
  ).toHaveAttribute("href", updateRelease.url);

  await page.mouse.move(0, 400);
  await expect(stale).toBeHidden();
  send({
    type: "update-check",
    status: {
      state: "error",
      message: "GitHub replied 403 Forbidden",
      checked_at: new Date().toISOString(),
    },
  });
  await page
    .getByRole("button", { name: "Couldn't check for updates" })
    .hover();
  await expect(
    page.getByRole("dialog", { name: "Couldn't check for updates" }),
  ).toContainText("GitHub replied 403 Forbidden");
});

test("the version card checks for updates on request", async ({ page }) => {
  const { messages, send } = await openApp(page);
  send({
    type: "update-check",
    status: {
      state: "up-to-date",
      latest: { ...updateRelease, version: "0.1.0" },
      checked_at: new Date(Date.now() - 3 * 3600_000).toISOString(),
    },
  });
  await page.getByRole("button", { name: "Stereo is up to date" }).click();
  const card = page.getByRole("dialog", { name: "Stereo is up to date" });
  const checkNow = card.getByRole("button", { name: "Check now" });
  await checkNow.click();
  const requests = () =>
    messages.filter((message) => message.type === "check-for-updates");
  await expect.poll(() => requests().length).toBe(1);
  await expect(checkNow).toHaveAttribute("aria-busy", "true");

  send({
    type: "error-reply",
    id: requests()[0].id,
    message: "update checks are off",
  });
  await expect(card).toContainText("Couldn't check: update checks are off");
  await expect(checkNow).not.toHaveAttribute("aria-busy", "true");
  // Focus stays on the button, so the outcome is announced.
  await expect(card.locator("div[role=status]")).toHaveText(
    "Couldn't check: update checks are off",
  );

  await checkNow.click();
  await expect.poll(() => requests().length).toBe(2);
  send({
    type: "update-check-reply",
    id: requests()[1].id,
    status: {
      state: "available",
      latest: updateRelease,
      checked_at: new Date().toISOString(),
    },
  });
  await expect(
    page.getByRole("button", { name: "vtest, Stereo v0.2.0 is available" }),
  ).toBeVisible();
  const updated = page.getByRole("dialog", {
    name: "Stereo v0.2.0 is available",
  });
  await expect(updated.locator("div[role=status]")).toHaveText(
    "Stereo v0.2.0 is available",
  );
  await expect(updated).not.toContainText("Couldn't check");
});

test("a manual update check outlasts a stalled GitHub request", async ({
  page,
}) => {
  await page.clock.install();
  const { messages, send } = await openApp(page);
  const status = (checked_at: string) => ({
    state: "up-to-date" as const,
    latest: { ...updateRelease, version: "0.1.0" },
    checked_at,
  });
  send({ type: "update-check", status: status(new Date().toISOString()) });
  await page.getByRole("button", { name: "Stereo is up to date" }).click();
  const card = page.getByRole("dialog", { name: "Stereo is up to date" });
  const checkNow = card.getByRole("button", { name: "Check now" });
  const requests = () =>
    messages.filter((message) => message.type === "check-for-updates");

  // The backend gives up on GitHub after 30 s; its reply still lands.
  await checkNow.click();
  await expect.poll(() => requests().length).toBe(1);
  await page.clock.runFor(31_000);
  await expect(checkNow).toHaveAttribute("aria-busy", "true");
  // The page's clock has run ahead of the test's.
  const failedAt = await page.evaluate(() => new Date().toISOString());
  send({
    type: "update-check-reply",
    id: requests()[0].id,
    status: {
      ...status(failedAt),
      refresh_error: {
        message: "operation timed out",
        failed_at: failedAt,
        is_stale: false,
      },
    },
  });
  await expect(card.locator("div[role=status]")).toContainText(
    "Checking again failed just now: operation timed out",
  );
  await expect(card).not.toContainText("didn't answer");

  // A backend that doesn't answer at all is reported, until a status arrives.
  await checkNow.click();
  await expect.poll(() => requests().length).toBe(2);
  await page.clock.runFor(46_000);
  await expect(card).toContainText("Couldn't check: The backend didn't answer.");
  send({ type: "update-check", status: status(failedAt) });
  await expect(card).not.toContainText("Couldn't check");
});

test(
  "the update details open from the keyboard",
  { tag: "@webkit" },
  async ({ page }) => {
    const { send } = await openApp(page);
    send({
      type: "update-check",
      status: {
        state: "available",
        latest: updateRelease,
        checked_at: new Date().toISOString(),
      },
    });
    const button = page.getByRole("button", {
      name: "Stereo v0.2.0 is available",
    });
    const card = page.getByRole("dialog", {
      name: "Stereo v0.2.0 is available",
    });
    await button.focus();
    await expect(card).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(card).toBeHidden();
    await expect(button).toBeFocused();
    // Pressing the focused button opens it again.
    await page.keyboard.press("Enter");
    await expect(card).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(card).toBeHidden();
    await page.keyboard.press("Space");
    await expect(card).toBeVisible();
  },
);

test.describe("on a touch screen", () => {
  test.use({ hasTouch: true });

  test(
    "a tap on the update icon shows the details",
    {
      tag: "@webkit",
    },
    async ({ page }) => {
      const { send } = await openApp(page);
      send({
        type: "update-check",
        status: {
          state: "error",
          message: "GitHub replied 403 Forbidden",
          checked_at: new Date().toISOString(),
        },
      });
      await page
        .getByRole("button", { name: "Couldn't check for updates" })
        .tap();
      await expect(
        page.getByRole("dialog", { name: "Couldn't check for updates" }),
      ).toContainText("GitHub replied 403 Forbidden");
    },
  );
});
