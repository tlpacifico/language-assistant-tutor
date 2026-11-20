import { useEffect } from "react";

const EDITOR_OR_DIALOG = 'input, textarea, select, [contenteditable], [aria-modal="true"]';

export function useArticleShortcuts({
  onRecord,
  onPauseToggle,
}: {
  onRecord: (() => void) | null;
  onPauseToggle: (() => void) | null;
}) {
  useEffect(() => {
    function handleKey(event: KeyboardEvent) {
      if (event.defaultPrevented || event.repeat || event.isComposing ||
          event.shiftKey || event.metaKey ||
          (event.target instanceof Element && event.target.closest(EDITOR_OR_DIALOG))) {
        return;
      }
      let action: (() => void) | null = null;
      if (event.ctrlKey && !event.altKey && event.key === "Enter") {
        action = onRecord;
      } else if (event.altKey && !event.ctrlKey && event.key.toLowerCase() === "p") {
        action = onPauseToggle;
      }
      if (action) {
        event.preventDefault();
        action();
      }
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [onRecord, onPauseToggle]);
}
