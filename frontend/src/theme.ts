import { defineTheme } from "@astryxdesign/core/theme";
import { neutralTheme } from "@astryxdesign/theme-neutral";

export const stereoTheme = defineTheme({
  name: "stereo",
  extends: neutralTheme,
  color: { accent: "#528fb3" },
  radius: { base: 2, multiplier: 1 },
});
