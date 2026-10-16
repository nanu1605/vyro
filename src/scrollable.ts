// A box whose content scrolls sideways (a wide table on a narrow window) must be reachable by
// keyboard, or its hidden columns are out of reach (WCAG 2.1.1). This action gives the box a tab
// stop only while its content is wider than it, so wide windows gain no extra stop. The box needs
// a role and a name of its own (`role="region"` and an `aria-label`).

export function scrollable(node: HTMLElement): { destroy(): void } {
  const update = () => {
    if (node.scrollWidth > node.clientWidth + 1) node.tabIndex = 0;
    else node.removeAttribute('tabindex');
  };
  const watch = new ResizeObserver(update);
  watch.observe(node);
  for (const child of node.children) watch.observe(child);
  update();
  return { destroy: () => watch.disconnect() };
}
