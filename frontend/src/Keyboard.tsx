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
      const target = event.target as HTMLElement;

      const isTyping =
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.isContentEditable;

      if (isTyping) return;

      let keyCombination = "";
      if (event.ctrlKey) keyCombination += "Ctrl+";
      if (event.metaKey) keyCombination += "Cmd+";
      if (event.altKey) keyCombination += "Alt+";
      if (event.shiftKey) keyCombination += "Shift+";

      const keyName = event.key === " " ? "Space" : event.key;
      keyCombination += keyName;

      const action =
        actionMapRef.current[keyCombination] || actionMapRef.current[keyName];

      if (action) {
        event.preventDefault();
        action();
      }
    };

    window.addEventListener("keydown", handleKeyDown);

    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);
};
