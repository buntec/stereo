// Stands in for the backend behind Vite's proxy in Playwright runs, for what the
// tests can't mock with routes: downloads, which Chromium doesn't route through
// Playwright.
export const fixturePort = Number(process.env.STEREO_FIXTURE_PORT ?? 8059);

/** The file served for `/collection-download/fixture`: a SQLite header. */
export const collectionFixture = "SQLite format 3\0fixture";

if (import.meta.main) {
  Bun.serve({
    port: fixturePort,
    fetch(request) {
      const { pathname } = new URL(request.url);
      if (pathname === "/collection-download/fixture")
        return new Response(collectionFixture, {
          headers: {
            "content-type": "application/vnd.sqlite3",
            "content-disposition":
              "attachment; filename=\"T_nze.db\"; filename*=UTF-8''T%C3%A4nze.db",
          },
        });
      if (pathname === "/") return new Response("Stereo test fixtures");
      return new Response("Not found", { status: 404 });
    },
  });
}
