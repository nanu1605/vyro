/** Placeholder shown for unavailable readings. */
export const NA = '—';

const cache = new Map<number, Intl.NumberFormat>();

function formatter(digits: number): Intl.NumberFormat {
  let f = cache.get(digits);
  if (!f) {
    f = new Intl.NumberFormat('en-US', { minimumFractionDigits: digits, maximumFractionDigits: digits });
    cache.set(digits, f);
  }
  return f;
}

/** Formats a reading, or {@link NA} when it is missing or non-finite. */
export function fmt(value: number | null | undefined, digits = 0): string {
  return value == null || !Number.isFinite(value) ? NA : formatter(digits).format(value);
}

/** Megabytes → gigabytes with one decimal. */
export function fmtGB(mb: number | null | undefined): string {
  return mb == null ? NA : fmt(mb / 1024, 1);
}
