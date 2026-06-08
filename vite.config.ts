import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// https://vite.dev/config/
export default defineConfig({
  build: {
    outDir: "dist",
  },
  publicDir: "frontend/public",
  server: {
    proxy: {
      "/ws": {
        target: `ws://localhost:${process.env.STEREO_PORT ?? "8005"}`,
        ws: true,
        rewriteWsOrigin: true,
      },
    },
  },
  plugins: [react()],
});
