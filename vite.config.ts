import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// https://vite.dev/config/
export default defineConfig({
  build: {
    chunkSizeWarningLimit: Infinity,
    outDir: "dist",
  },
  publicDir: "frontend/public",
  server: {
    port: 5174,
    strictPort: true,
    proxy: {
      "/ws": {
        target: `ws://localhost:${process.env.STEREO_PORT ?? "8005"}`,
        ws: true,
      },
      "/audio": `http://localhost:${process.env.STEREO_PORT ?? "8005"}`,
      "/collection-download": `http://localhost:${process.env.STEREO_PORT ?? "8005"}`,
    },
  },
  plugins: [react()],
});
