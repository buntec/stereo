import { useEffect, useRef } from "react";

type KeyActionMap = Record<string, () => void>;

// Text entry and open overlays (dialogs, menus, listboxes) receive every key.
const TEXT_ENTRY_OR_OVERLAY = [
  'input:not([type="checkbox"]):not([type="radio"]):not([type="button"]):not([type="submit"]):not([type="reset"]):not([type="range"])',
  "textarea",
  "select",
  '[contenteditable]:not([contenteditable="false"])',
  '[role="textbox"]',
  '[role="searchbox"]',
  '[role="combobox"]',
  '[role="spinbutton"]',
  '[role="dialog"]',
  '[role="alertdialog"]',
  '[role="menu"]',
  '[role="menubar"]',
  '[role="listbox"]',
].join(", ");

// Focused controls keep Space/Enter for activation.
const ACTIVATABLE = [
  "button",
  "a[href]",
  "input",
  '[role="button"]',
  '[role="link"]',
  '[role="checkbox"]',
  '[role="radio"]',
  '[role="switch"]',
  '[role="menuitem"]',
  '[role="tab"]',
  '[role="option"]',
].join(", ");

// Composite widgets keep arrow-style navigation keys.
const NAVIGABLE = [
  '[role="slider"]',
  '[role="radiogroup"]',
  '[role="grid"]',
  '[role="treegrid"]',
  '[role="tablist"]',
].join(", ");

const NAVIGATION_KEYS = new Set([
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "Home",
  "End",
  "PageUp",
  "PageDown",
]);

// Whether the focused element needs this key for its own behavior. Everything else
// (letters, digits, Backspace, ...) stays a global shortcut, so shortcuts keep working
// after clicking a button or a grid row.
const isOwnedByTarget = (target: Element, key: string): boolean => {
  if (target.closest(TEXT_ENTRY_OR_OVERLAY)) return true;
  if ((key === " " || key === "Enter") && target.closest(ACTIVATABLE))
    return true;
  if (NAVIGATION_KEYS.has(key) && target.closest(NAVIGABLE)) return true;
  return false;
};

export const useKeyboardActions = (actionMap: KeyActionMap) => {
  const actionMapRef = useRef<KeyActionMap>(actionMap);

  // this prevents the second useEffect from firing when the actionMap prop changes
  useEffect(() => {
    actionMapRef.current = actionMap;
  }, [actionMap]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent): void => {
      const target = event.target;
      if (
        event.defaultPrevented ||
        event.isComposing ||
        !(target instanceof Element)
      )
        return;
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      if (isOwnedByTarget(target, event.key)) return;

      const keyName = event.key === " " ? "Space" : event.key;
      const keyCombination = `${event.shiftKey ? "Shift+" : ""}${keyName}`;

      const action = actionMapRef.current[keyCombination];

      if (action) {
        event.preventDefault();
        action();
      }
    };

    window.addEventListener("keydown", handleKeyDown);

    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);
};
