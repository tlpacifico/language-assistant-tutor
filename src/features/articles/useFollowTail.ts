import { useLayoutEffect, useRef, useState, type RefObject } from "react";

const END_THRESHOLD = 80;

export function useFollowTail(
  ref: RefObject<HTMLElement | null>,
  turnIds: readonly string[],
) {
  const [atEnd, setAtEnd] = useState(true);
  const [newCount, setNewCount] = useState(0);
  const following = useRef(true);
  const previous = useRef<readonly string[] | null>(null);

  function scrollToEnd(behavior: ScrollBehavior) {
    const region = ref.current;
    if (!region) return;
    const top = Math.max(0, region.scrollHeight - region.clientHeight);
    if (typeof region.scrollTo === "function") {
      region.scrollTo({ top, behavior });
    } else {
      region.scrollTop = top;
    }
  }

  function onScroll() {
    const region = ref.current;
    if (!region) return;
    const closeToEnd =
      region.scrollHeight - region.clientHeight - region.scrollTop <= END_THRESHOLD;
    following.current = closeToEnd;
    setAtEnd(closeToEnd);
    if (closeToEnd) setNewCount(0);
  }

  useLayoutEffect(() => {
    const last = previous.current;
    previous.current = turnIds;
    if (!last) {
      scrollToEnd("auto");
      return;
    }
    const added = turnIds.filter((id) => !last.includes(id)).length;
    if (!added) return;
    if (following.current) {
      scrollToEnd("auto");
    } else {
      setNewCount((count) => count + added);
    }
  }, [turnIds]);

  function jumpToLatest() {
    following.current = true;
    setAtEnd(true);
    setNewCount(0);
    const reducedMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    scrollToEnd(reducedMotion ? "auto" : "smooth");
  }

  return { atEnd, newCount, jumpToLatest, onScroll };
}
