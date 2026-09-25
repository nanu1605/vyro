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

/** "3d 4h 12m", "4h 12m", "12m 5s". */
export function fmtDuration(secs: number | null | undefined): string {
  if (secs == null || !Number.isFinite(secs) || secs < 0) return NA;
  const s = Math.floor(secs);
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d}d ${h}h ${m}m`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m ${s % 60}s`;
}

/** Local date and time for a Unix timestamp (seconds). */
export function fmtDateTime(unix: number | null | undefined): string {
  if (unix == null) return NA;
  return new Date(unix * 1000).toLocaleString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

/** Local date for a Unix timestamp (seconds). */
export function fmtDate(unix: number | null | undefined): string {
  if (unix == null) return NA;
  return new Date(unix * 1000).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
}

/** Cache / memory sizes from KB: "48 KB", "2 MB", "36 MB". */
export function fmtKB(kb: number | null | undefined): string {
  if (kb == null) return NA;
  return kb >= 1024 ? `${fmt(kb / 1024, kb % 1024 === 0 ? 0 : 1)} MB` : `${fmt(kb)} KB`;
}

/** Memory from MB: "16 GB", "512 MB". */
export function fmtMB(mb: number | null | undefined): string {
  if (mb == null) return NA;
  return mb >= 1024 ? `${fmt(mb / 1024, mb % 1024 === 0 ? 0 : 1)} GB` : `${fmt(mb)} MB`;
}

/** Four-digit uppercase hex, e.g. PCI ids. */
export function hex4(v: number): string {
  return v.toString(16).toUpperCase().padStart(4, '0');
}
