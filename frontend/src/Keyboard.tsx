import { useEffect, useRef } from "react";

type KeyActionMap = Record<string, () => void>;

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
      // Controls and composite widgets own their native keyboard behavior.
      if (
        target.closest(
          'input, textarea, select, button, a, [contenteditable]:not([contenteditable="false"]), [role="button"], [role="slider"], [role="radio"], [role="grid"], [role="menu"], [role="listbox"], [role="dialog"], [role="alertdialog"]',
        )
      )
        return;
      if (event.metaKey || event.ctrlKey || event.altKey) return;

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
