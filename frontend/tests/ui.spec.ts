import {
  test,
  expect,
  type Locator,
  type Page,
  type Route,
  type WebSocketRoute,
} from "@playwright/test";

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
    /** Leaves `validate-track` unanswered, for the test to answer itself. */
    holdValidation?: boolean;
  } = {},
) {
  let socket: WebSocketRoute;
  const tracks = options.empty ? [] : structuredClone(exampleTracks);
  const collection = { path: "/audit/collection.db", size: tracks.length };
  const messages: Record<string, unknown>[] = [];
  /** Ids the collection answers `get-collection-track` as not containing. */
  const absent = new Set<string>();
  await page.route("https://www.youtube.com/**", (route) => route.abort());
  await page.route("https://i.ytimg.com/**", (route) => route.abort());
  const audioRequests: string[] = [];
  /** Requests answered with `hold`, which a test may still answer itself. */
  const heldAudio: Route[] = [];
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
        case "get-rows":
          if (!options.holdRows)
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
          break;
        case "set-repo":
          send({
            type: "collections",
            collections: options.noCollection ? [] : [collection],
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
          { type: "repos", repos: [{ path: "/audit", remote: null }] },
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

test("the disco ball hangs still unless asked to turn", async ({ page }) => {
  await openApp(page);
  const still = await discoBallPixels(page);
  await page.waitForTimeout(300);
  expect(await discoBallPixels(page)).toBe(still);

  await setDiscoBallSlider(page, "Rotation speed", "End");
  await expect
    .poll(() => discoBallPixels(page), { timeout: 2000 })
    .not.toBe(still);
});

test("the disco ball fits a short window, which keeps its cord setting", async ({
  page,
}) => {
  await page.setViewportSize({ width: 667, height: 375 });
  await openApp(page);
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
    // The cord, the cap (6px), the ball (40% of 900px) and its margin (4px).
    .toBe(400 + 6 + 360 + 4);
});

test("the disco ball shrinks below its smallest size to fit, then isn't drawn", async ({
  page,
}) => {
  await openApp(page);
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
  await openApp(page, { empty: true });
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
  await openApp(page);
  await expect(page.locator("canvas.disco-ball")).toHaveClass(
    /disco-ball-subdued/,
  );
});

test("motion settings say they're off for those who prefer reduced motion", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openApp(page);
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
  await openApp(page);
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
  const background = page.getByRole("button", {
    name: "Toggle background artwork",
    exact: true,
  });
  const shuffle = page.getByRole("button", {
    name: "Toggle shuffle play",
    exact: true,
  });
  await page.locator(".play-pause-button").click();
  await page.keyboard.press("b");
  await expect(background).toHaveAttribute("aria-pressed", "false");
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
  await page
    .getByRole("button", { name: "Toggle background artwork", exact: true })
    .click();
  await expect(background).toHaveCount(0);
});

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
