import "@fontsource-variable/figtree";
import {
  defineTheme,
  type DefinedTheme,
  type TokenName,
} from "@astryxdesign/core/theme";
import { neutralTheme } from "@astryxdesign/theme-neutral";

// Neutral names Figtree without loading it: it's self-hosted by the import
// above, which registers it as "Figtree Variable".
const fontTokens = Object.fromEntries(
  Object.entries(neutralTheme.tokens)
    .filter(([token]) => token.startsWith("--font-family-"))
    .map(([token, stack]) => [
      token,
      stack.replace(
        /(^|,\s*)Figtree(?=\s*,|$)/,
        '$1"Figtree Variable", Figtree',
      ),
    ]),
) as Partial<Record<TokenName, string>>;

export const stereoTheme: DefinedTheme = defineTheme({
  name: "stereo",
  extends: neutralTheme,
  tokens: fontTokens,
});
