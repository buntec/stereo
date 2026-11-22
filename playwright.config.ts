import { defineConfig } from "@playwright/test";
import { fixturePort } from "./frontend/tests/fixture-server.ts";

export default defineConfig({
  testDir: "./frontend/tests",
  fullyParallel: true,
  use: {
    baseURL: "http://127.0.0.1:5174",
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: {
        browserName: "chromium",
        launchOptions: {
          executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE,
        },
      },
    },
    // The desktop app's engine, for tests of what differs there (tagged @webkit).
    { name: "webkit", grep: /@webkit/, use: { browserName: "webkit" } },
  ],
  webServer: [
    {
      // What Vite proxies to the backend (see fixture-server.ts).
      command: "bun frontend/tests/fixture-server.ts",
      url: `http://127.0.0.1:${fixturePort}/`,
      reuseExistingServer: !process.env.CI,
    },
    {
      command: "bun run dev --host 127.0.0.1",
      url: "http://127.0.0.1:5174",
      env: { STEREO_PORT: String(fixturePort) },
      reuseExistingServer: !process.env.CI,
    },
  ],
});
