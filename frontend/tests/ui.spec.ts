import {
  test,
  expect,
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
  } = {},
) {
  let socket: WebSocketRoute;
  const tracks = options.empty ? [] : structuredClone(exampleTracks);
  const collection = { path: "/audit/collection.db", size: tracks.length };
  const messages: Record<string, unknown>[] = [];
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
            { type: "search-results", query_id: message.query_id, tracks: [] },
            { type: "search-complete", query_id: message.query_id },
          ]);
          break;
        case "validate-track":
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
    page.getByText("Connecting to Stereo", { exact: true }),
  ).toHaveCount(0);
  return {
    messages,
    audioRequests,
    heldAudio,
    /** Changes how further audio requests are answered (not "off"). */
    setAudio: (mode: "hold" | "ok" | "fail") => {
      audio = mode;
    },
    send: (message: unknown) => socket.send(JSON.stringify(message)),
  };
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

test("rating supports arrow keys and an explicit clear action", async ({
  page,
}) => {
  const { messages } = await openApp(page);
  const row = page.locator('.ag-row[row-index="0"]');
  const group = row.getByRole("radiogroup");
  await group.getByRole("radio", { name: "4 of 5 stars" }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(
    group.getByRole("radio", { name: "5 of 5 stars" }),
  ).toHaveAttribute("aria-checked", "true");
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
  await expect(page.getByText("First warning")).toBeVisible();
  await expect(page.getByText("Second warning")).toBeVisible();
  send({ type: "notification", kind: "info", message: "First information" });
  await expect(page.getByText("First information")).toBeVisible();
  await page.waitForTimeout(4200);
  send({ type: "notification", kind: "info", message: "Later information" });
  await page.waitForTimeout(1200);
  await expect(page.getByText("Later information")).toBeVisible();
});

for (const width of [390, 768, 1440]) {
  test(`header and playback stay usable at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 844 });
    await openApp(page);
    await expect(page.getByRole("main")).toHaveCount(1);
    for (const control of [
      page.locator(".collection-controls").getByRole("button").first(),
      page.getByRole("button", { name: /^YouTube:/ }),
      page.getByRole("button", { name: /^Spotify:/ }),
      page.getByRole("button", { name: /^Beatport:/ }),
      page.getByRole("radiogroup", { name: "Appearance" }),
      page.getByRole("button", { name: "Search", exact: true }),
      page.getByRole("button", { name: "Toggle shuffle play", exact: true }),
    ]) {
      await expect(control).toBeInViewport();
      const box = await control.boundingBox();
      expect(box!.x).toBeGreaterThanOrEqual(0);
      expect(box!.x + box!.width).toBeLessThanOrEqual(width);
    }
    const grid = await page.locator(".grid").boundingBox();
    expect(grid!.height).toBeGreaterThan(300);
  });
}

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
    page.getByRole("heading", { name: "Your collection is ready" }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Search for music", exact: true })
    .click();
  await expect(
    page.getByRole("textbox", { name: /Search query/ }),
  ).toBeFocused();
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

test("metadata editing groups identifiers and explains invalid required fields", async ({
  page,
}) => {
  await openApp(page);
  await page
    .getByRole("button", { name: "Edit track", exact: true })
    .first()
    .click();
  const dialog = page.getByRole("dialog");
  await expect(
    dialog.getByRole("button", { name: "Fill missing metadata" }),
  ).toBeVisible();
  await expect(
    dialog.getByRole("textbox", { name: "Spotify ID", exact: true }),
  ).toBeHidden();
  await dialog.getByRole("textbox", { name: /^Title/ }).fill("");
  await expect(dialog.getByText("Enter a track title.")).toBeVisible();
  await expect(
    dialog.getByRole("button", { name: "Save", exact: true }),
  ).toBeDisabled();
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

test("slider arrow keys seek one slider step instead of triggering global shortcuts", async ({
  page,
}) => {
  await openApp(page, { audio: "ok", audioSeconds: 240 });
  const slider = page.getByRole("slider", {
    name: "Playback progress",
    exact: true,
  });
  await expect(slider).toBeEnabled();
  await page.locator(".play-pause-button").click();
  await expect(page.locator(".play-pause-button")).toHaveAccessibleName("Play");
  const before = Number(await slider.getAttribute("aria-valuenow"));
  await slider.focus();
  await page.keyboard.press("ArrowRight");
  // One step is 1% of the 4 minutes, and the arrow key doesn't also skip tracks.
  await expect
    .poll(async () => Number(await slider.getAttribute("aria-valuenow")))
    .toBeCloseTo(before + 1, 0);
  await expect(page.getByText("00:02", { exact: true })).toBeVisible();
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

test("plays downloaded audio, prefetching and advancing on its own", async ({
  page,
}) => {
  const { audioRequests } = await openApp(page, { audio: "ok" });
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
  await expect(
    page
      .getByText(
        "Couldn't download this track's audio, skipping it: yt-dlp failed",
        { exact: true },
      )
      .first(),
  ).toBeVisible();
  await expect(
    page.getByText(
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
  await expect.poll(() => heldAudio.length).toBeGreaterThan(0);
  const button = page.locator(".play-pause-button");
  await button.click();
  await expect(button).toHaveAccessibleName("Play");
  setAudio("fail");
  for (const route of heldAudio.splice(0))
    await route.fulfill({ status: 502, body: "yt-dlp failed" });
  await expect(
    page.getByText("Couldn't download this track's audio: yt-dlp failed", {
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
  await expect(
    page.getByText(
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

test("explains that playback needs yt-dlp when the backend can't download audio", async ({
  page,
}) => {
  await openApp(page, { audio: "off" });
  await expect(
    page.getByText("Playback needs yt-dlp", { exact: true }),
  ).toBeVisible();
});

test("shows the playing track's download progress in place of the playback slider", async ({
  page,
}) => {
  const { send, audioRequests } = await openApp(page);
  await expect
    .poll(() => audioRequests)
    .toContain("GET /audio/audit0?key=test-key");
  const slider = page.getByRole("slider", { name: "Playback progress" });
  const download = page.getByRole("progressbar", {
    name: "Audio download progress",
  });
  await expect(slider).toBeVisible();

  // Downloads of other tracks (prefetching) aren't shown.
  send({
    type: "audio-download",
    yt_id: "audit1",
    status: "downloading",
    progress: 0.9,
  });
  send({
    type: "audio-download",
    yt_id: "audit0",
    status: "preparing",
    progress: null,
  });
  await expect(
    page.getByText("Preparing download", { exact: true }),
  ).toBeVisible();
  await expect(slider).toBeHidden();

  send({
    type: "audio-download",
    yt_id: "audit0",
    status: "downloading",
    progress: 0.43,
  });
  await expect(download).toHaveAttribute("aria-valuenow", "43");
  await expect(page.getByText("43%", { exact: true })).toBeVisible();

  send({
    type: "audio-download",
    yt_id: "audit0",
    status: "done",
    progress: null,
  });
  await expect(download).toBeHidden();
  await expect(slider).toBeVisible();
});

test("the play button shows that playback is loading and can cancel it", async ({
  page,
}) => {
  const { audioRequests } = await openApp(page);
  await expect
    .poll(() => audioRequests)
    .toContain("GET /audio/audit0?key=test-key");
  const button = page.locator(".play-pause-button");
  await expect(button).toHaveAttribute("aria-busy", "true");
  await expect(button).toHaveAccessibleName("Pause");
  // Nothing to seek in before the duration is known.
  await expect(
    page.getByRole("slider", { name: "Playback progress", exact: true }),
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
