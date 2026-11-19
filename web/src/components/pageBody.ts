/**
 * Where page-level overlays mount. In place, a trigger's parent would style them through child
 * selectors, and a face would take their scrolls and presses.
 */
export function pageBody(): HTMLElement | null {
  return typeof document === "undefined" ? null : document.body;
}
