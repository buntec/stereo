import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// https://vite.dev/config/
export default defineConfig({
  build: {
    outDir: "src/stereo/statics",
  },
  publicDir: "frontend/public",
  server: {
    proxy: {
      "/ws": {
        target: "ws://localhost:8005",
        ws: true,
        rewriteWsOrigin: true,
      },
    },
  },
  plugins: [react()],
});
