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
    // Translucent, frosted-glass toasts instead of solid inverted surfaces.
    toast: {
      base: {
        backdropFilter: "blur(16px) saturate(180%)",
        WebkitBackdropFilter: "blur(16px) saturate(180%)",
        borderWidth: "1px",
        borderStyle: "solid",
        borderColor:
          "color-mix(in srgb, var(--color-on-dark) 15%, transparent)",
      },
      "type:info": {
        backgroundColor:
          "color-mix(in srgb, var(--color-background-inverted) 55%, transparent)",
      },
      "type:error": {
        backgroundColor:
          "color-mix(in srgb, var(--color-background-error-inverted) 55%, transparent)",
      },
    },
  },
});
