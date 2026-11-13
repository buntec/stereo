/**
 * The playing track's progress (0–1) for its grid row's progress bar
 * (`--stereo-playback-progress`, read by `.grid .progress-bar` in App.css).
 * Set by a rule of its own rather than on the document: a custom property
 * changed on the root is inherited by every element, and recomputing all of
 * their styles takes some 20ms on a full grid, every second, which visibly
 * stalls the animations timed by the beats. Changing this rule restyles only
 * the playing row.
 */
let rule: CSSStyleRule | undefined;

export const setRowProgress = (fraction: number) => {
  if (!rule) {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(".progress-bar {}");
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
    rule = sheet.cssRules[0] as CSSStyleRule;
  }
  rule.style.setProperty("--stereo-playback-progress", `${fraction}`);
};
