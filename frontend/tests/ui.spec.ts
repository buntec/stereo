import { test, expect, type Page, type WebSocketRoute } from "@playwright/test";

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
    playback?: boolean;
  } = {},
) {
  let socket: WebSocketRoute;
  const tracks = options.empty ? [] : structuredClone(exampleTracks);
  const collection = { path: "/audit/collection.db", size: tracks.length };
  const messages: Record<string, unknown>[] = [];
  await page.route("https://www.youtube.com/**", (route) => route.abort());
  await page.route("https://i.ytimg.com/**", (route) => route.abort());
  if (options.playback)
    await page.route("https://www.youtube.com/iframe_api", (route) =>
      route.fulfill({
        contentType: "text/javascript",
        body: `
      window.__playerMounts = 0;
      window.__playerDestroys = 0;
      class Player {
        constructor(node, options) {
          this.events = options.events; this.id = ''; this.time = 0; this.state = -1;
          this.iframe = document.createElement('iframe');
          this.iframe.title = 'YouTube player'; this.iframe.width = '640'; this.iframe.height = '360';
          node.replaceWith(this.iframe); window.__playerMounts++;
          setTimeout(() => this.events.onReady({target: this}), 0);
        }
        getVideoData() { return {video_id: this.id, title: 'Example track 1'}; }
        getCurrentTime() { return this.time; }
        getDuration() { return 240; }
        getPlayerState() { return this.state; }
        emit(state) { this.state = state; this.events.onStateChange({target: this, data: state}); }
        loadVideoById(id) { this.id = id; this.emit(1); }
        playVideo() { this.emit(1); }
        pauseVideo() { this.emit(2); }
        stopVideo() { this.emit(-1); }
        seekTo(time) { this.time = time; window.__lastSeek = time; }
        destroy() { window.__playerDestroys++; this.iframe.remove(); }
      }
      window.YT = {Player, PlayerState: {UNSTARTED:-1, ENDED:0, PLAYING:1, PAUSED:2, BUFFERING:3, CUED:5}};
      window.onYouTubeIframeAPIReady();
    `,
      }),
    );
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
          { type: "backend-info", version: "test", home_dir: "/audit" },
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
    send: (message: unknown) => socket.send(JSON.stringify(message)),
  };
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

async function enableVideo(page: Page) {
  await page.addInitScript(() =>
    localStorage.setItem(
      "stereo_app_settings",
      JSON.stringify({
        appearance: "dark",
        repoPath: "/audit",
        collectionPath: "/audit/collection.db",
        video: true,
        backgroundVideo: true,
        shufflePlay: false,
        fullScreen: false,
      }),
    ),
  );
}

async function expectUnclippedVideo(page: Page) {
  const player = (await page.locator(".player").boundingBox())!;
  const iframe = (await page
    .locator(".player iframe[title='YouTube player']")
    .boundingBox())!;
  const footer = (await page.locator(".playback-footer").boundingBox())!;
  // The iframe fills a 16:9 frame that ends above the transport footer.
  expect(Math.abs(player.width / player.height - 16 / 9)).toBeLessThan(0.02);
  expect(iframe).toEqual(player);
  expect(player.y).toBeGreaterThanOrEqual(0);
  expect(player.y + player.height).toBeLessThanOrEqual(footer.y);
  return player;
}

for (const [width, height] of [
  [1280, 720],
  [390, 844],
]) {
  test(`video is sized to fit and leaves room for the grid at ${width}x${height}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height });
    await enableVideo(page);
    await openApp(page, { playback: true });
    await expect(
      page.getByRole("slider", { name: "Playback progress", exact: true }),
    ).toBeEnabled();
    const player = await expectUnclippedVideo(page);
    expect(player.x + player.width).toBeLessThanOrEqual(width);
    const grid = (await page.locator(".grid").boundingBox())!;
    expect(grid.height).toBeGreaterThan(200);
  });
}

test("fullscreen enlarges the video and preserves player instances and grid selection", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await enableVideo(page);
  await openApp(page, { playback: true });
  await expect(
    page.getByRole("slider", { name: "Playback progress", exact: true }),
  ).toBeEnabled();
  await page.locator('.ag-row[row-index="0"]').getByRole("checkbox").check();
  await expect(page.getByText("1 selected", { exact: true })).toBeVisible();
  const windowed = await expectUnclippedVideo(page);
  await page
    .getByRole("button", { name: "Toggle fullscreen mode", exact: true })
    .click();
  await expect(page.locator(".grid")).toBeHidden();
  await expect(
    page.getByRole("heading", { name: "Example track 1", exact: true }),
  ).toBeVisible();
  const fullscreen = await expectUnclippedVideo(page);
  expect(fullscreen.width).toBeGreaterThan(windowed.width);
  await page
    .getByRole("button", { name: "Toggle fullscreen mode", exact: true })
    .click();
  await expect(page.locator(".grid")).toBeVisible();
  await expect(page.getByText("1 selected", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => Reflect.get(window, "__playerMounts"))).toBe(
    2,
  );
  expect(
    await page.evaluate(() => Reflect.get(window, "__playerDestroys")),
  ).toBe(0);
});

test("single-key shortcuts still work after clicking a button or a grid row", async ({
  page,
}) => {
  await openApp(page, { playback: true });
  const video = page.getByRole("button", { name: "Toggle video", exact: true });
  const shuffle = page.getByRole("button", {
    name: "Toggle shuffle play",
    exact: true,
  });
  await page.locator(".play-pause-button").click();
  await page.keyboard.press("v");
  await expect(video).toHaveAttribute("aria-pressed", "true");
  await page.locator('.ag-row[row-index="2"] [col-id="title"]').click();
  await page.keyboard.press("s");
  await expect(shuffle).toHaveAttribute("aria-pressed", "true");
});

test("slider arrow keys seek one slider step instead of triggering global shortcuts", async ({
  page,
}) => {
  await openApp(page, { playback: true });
  const slider = page.getByRole("slider", {
    name: "Playback progress",
    exact: true,
  });
  await expect(slider).toBeEnabled();
  await slider.focus();
  await page.keyboard.press("ArrowRight");
  await expect
    .poll(() => page.evaluate(() => Reflect.get(window, "__lastSeek")))
    .toBe(2.4);
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
