import js from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import tseslint from "typescript-eslint";
import { defineConfig, globalIgnores } from "eslint/config";

export default defineConfig([
  globalIgnores([
    "**/dist/**",
    "**/node_modules/**",
    "**/target/**",
    "**/vendor/**",
    "**/vendors/**",
    "**/third_party/**",
    "**/third-party/**",
    "**/generated/**",
    "src/stereo/statics",
  ]),
  {
    files: ["**/*.{ts,tsx}"],
    extends: [
      js.configs.recommended,
      tseslint.configs.recommended,
      reactHooks.configs.flat.recommended,
      reactRefresh.configs.vite,
    ],
    languageOptions: {
      ecmaVersion: 2020,
      globals: globals.browser,
    },
  },
  {
    files: [
      "frontend/src/CollectionSelector.tsx",
      "frontend/src/YouTubeSessionDialog.tsx",
    ],
    rules: {
      "react-refresh/only-export-components": "off",
    },
  },
  {
    files: [
      "frontend/src/EditDialog.tsx",
      "frontend/src/GitRemoteDialog.tsx",
      "frontend/src/SpotifyPlaylistDialog.tsx",
    ],
    rules: {
      "react-hooks/set-state-in-effect": "off",
    },
  },
]);
