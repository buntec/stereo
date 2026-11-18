//! On macOS the page reaches up under a transparent title bar (`TitleBarStyle::Overlay`), so the
//! background artwork fills the whole window, when the page leaves the title bar's strip clear:
//! the start page, and the UI of a backend whose `/info` lists `FEATURE`. Older backends' UIs get
//! the native title bar back (`set_overlay`).
//!
//! The page sees `data-title-bar` on its root element: `overlay` while the title bar is over it,
//! else `hidden` (a native title bar, or full screen, where it only slides in over the page). The
//! strip is the window's title bar: `INIT_SCRIPT` moves the window from it, and on a double-click
//! does what the user chose in System Settings (Desktop & Dock), through the window commands in
//! `capabilities/default.json`. Tauri's `data-tauri-drag-region` always zooms instead.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{Manager, TitleBarStyle, WebviewWindow};

/// What a backend's `/info` lists among its `features` when its UI leaves the strip clear.
pub const FEATURE: &str = "title-bar-overlay";

/// Whether the window's title bar is over the page. The window starts with it, on the start page.
pub struct TitleBar(AtomicBool);

impl Default for TitleBar {
    fn default() -> Self {
        TitleBar(AtomicBool::new(true))
    }
}

/// Run in every page before its own scripts. The strip's height is the title bar's, 28 px, as in
/// the UI's App.css. Its clicks on anything interactive (as Tauri's `drag.js` tells) are left to
/// the page. A double-click acts on release, unless the pointer moved, as the native title bar's.
pub fn init_script() -> String {
    format!(
        r#"window.__STEREO_DESKTOP__ = true;
(() => {{
  const root = document.documentElement;
  root.dataset.titleBar = "overlay";
  window.__STEREO_TITLE_BAR_DOUBLE_CLICK__ = "{action}";
  const HEIGHT = 28;
  const INTERACTIVE =
    "a, button, input, select, textarea, label, summary, " +
    "[contenteditable]:not([contenteditable='false']), [tabindex]:not([tabindex='-1']), " +
    "[role=button], [role=link], [role=menuitem], [role=tab], [role=checkbox], " +
    "[role=radio], [role=switch], [role=option]";
  const invoke = (command) =>
    window.__TAURI_INTERNALS__
      .invoke("plugin:window|" + command)
      .catch((error) => console.warn("title bar:", command, error));
  const inTitleBar = (event) =>
    root.dataset.titleBar === "overlay" &&
    event.button === 0 &&
    event.clientY < HEIGHT &&
    !event.composedPath().some((el) => el instanceof Element && el.matches(INTERACTIVE));
  let doubleClickAt = null;
  document.addEventListener("mousedown", (event) => {{
    doubleClickAt = null;
    if (!inTitleBar(event)) return;
    event.preventDefault();
    if (event.detail === 1) invoke("start_dragging");
    else if (event.detail === 2) doubleClickAt = [event.clientX, event.clientY];
  }});
  document.addEventListener("mouseup", (event) => {{
    const at = doubleClickAt;
    doubleClickAt = null;
    if (!at || event.detail !== 2 || !inTitleBar(event)) return;
    if (event.clientX !== at[0] || event.clientY !== at[1]) return;
    const action = window.__STEREO_TITLE_BAR_DOUBLE_CLICK__;
    if (action === "minimize") invoke("minimize");
    else if (action === "zoom") invoke("toggle_maximize");
  }});
}})();
"#,
        action = double_click_action()
    )
}

/// Puts the title bar over the page, or back above it, e.g. before opening a backend's UI.
pub fn set_overlay(window: &WebviewWindow, overlay: bool) {
    window
        .state::<TitleBar>()
        .0
        .store(overlay, Ordering::SeqCst);
    let style = if overlay {
        TitleBarStyle::Overlay
    } else {
        TitleBarStyle::Visible
    };
    if let Err(error) = window.set_title_bar_style(style) {
        eprintln!("could not set the title bar style: {error}");
    }
    sync(window);
}

/// Tells the page whether the title bar is over it, and what a double-click on it does: after
/// each page load, resize (e.g. into full screen) and focus (back from System Settings).
pub fn sync(window: &WebviewWindow) {
    let overlay = window.state::<TitleBar>().0.load(Ordering::SeqCst)
        && !window.is_fullscreen().unwrap_or(false);
    let state = if overlay { "overlay" } else { "hidden" };
    let action = double_click_action();
    let _ = window.eval(format!(
        "document.documentElement.dataset.titleBar = '{state}';\
         window.__STEREO_TITLE_BAR_DOUBLE_CLICK__ = '{action}';"
    ));
}

/// What double-clicking a title bar does, from System Settings (Desktop & Dock).
fn double_click_action() -> &'static str {
    use objc2_foundation::{ns_string, NSUserDefaults};

    let defaults = NSUserDefaults::standardUserDefaults();
    let action = defaults
        .stringForKey(ns_string!("AppleActionOnDoubleClick"))
        .map(|action| action.to_string());
    let minimize = defaults.boolForKey(ns_string!("AppleMiniaturizeOnDoubleClick"));
    action_for(action.as_deref(), minimize)
}

/// `AppleActionOnDoubleClick` is `Maximize` (Zoom), `Fill`, `Minimize` or `None`; without it, the
/// older `AppleMiniaturizeOnDoubleClick` says whether to minimize, else it zooms. Fill zooms too:
/// the window's zoom already fills the screen, as it has no maximum size.
fn action_for(action: Option<&str>, minimize: bool) -> &'static str {
    match action {
        Some("Minimize") => "minimize",
        Some("None") => "none",
        Some(_) => "zoom",
        None if minimize => "minimize",
        None => "zoom",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_double_click_action() {
        assert_eq!(action_for(None, false), "zoom");
        assert_eq!(action_for(None, true), "minimize");
        assert_eq!(action_for(Some("Maximize"), true), "zoom");
        assert_eq!(action_for(Some("Fill"), false), "zoom");
        assert_eq!(action_for(Some("Minimize"), false), "minimize");
        assert_eq!(action_for(Some("None"), true), "none");
    }
}
