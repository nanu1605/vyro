// How the phone app moves (1.7): short and eased, and not at all with reduced motion.

const reduce = typeof matchMedia === 'function' ? matchMedia('(prefers-reduced-motion: reduce)') : null;

/** `ms`, or 0 when the phone asks for reduced motion. */
export function ms(n: number): number {
  return reduce?.matches ? 0 : n;
}

/** Where a tab sits in the bar decides the side a new view comes from: -1, 0 or 1. */
export function direction(from: number, to: number): number {
  return Math.sign(to - from);
}
