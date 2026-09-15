import { defineTheme } from "@astryxdesign/core/theme";
import { neutralTheme } from "@astryxdesign/theme-neutral";

export const stereoTheme = defineTheme({
  name: "stereo",
  extends: neutralTheme,
  color: { accent: "#528fb3" },
  radius: { base: 2, multiplier: 1 },
  components: {
    // Menus mark selected options with a trailing check (MenuCheck in
    // Menu.tsx) instead of radio circles and checkbox squares.
    "dropdown-menu-radio": { base: { display: "none" } },
    "dropdown-menu-checkbox": { base: { display: "none" } },
  },
});
