const PREROLL_MARGIN = "128px";

export function watchOnScreen(element: Element, onScreen: (visible: boolean) => void): () => void {
  if (typeof IntersectionObserver === "undefined") {
    onScreen(true);
    return () => {};
  }
  const observer = new IntersectionObserver(
    (entries) => onScreen(entries[entries.length - 1]?.isIntersecting ?? false),
    { rootMargin: PREROLL_MARGIN },
  );
  observer.observe(element);
  return () => observer.disconnect();
}
