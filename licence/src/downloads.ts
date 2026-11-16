// Clokrix's downloads and updates (2.5.2), from the company's own website rather than GitHub, so
// the releases repository can be private (the owner, 2026-11-16: "make the vyro repo private and
// also make the vyro-release private", "If vyro-release become private how can we distribute
// updates?"). The Release workflow puts each release's files in the R2 bucket bound as DOWNLOADS;
// this serves them:
//   GET /download                  the newest installer (a redirect to it), for the website's
//                                  Download button
//   GET /download/<tag>/<file>     a release's installer or checksums (tag: vX.Y.Z)
//   GET /updates/latest.json       what installed copies' updater reads: the newest version, its
//                                  installer's address and the installer's update signature
// The manifest names only the newest release, so a copy several versions behind goes straight to
// it, as from GitHub before: each installer is the whole of Clokrix.
// On every address the Worker answers on, like the API, and before the ledger's rate limit: an
// update check is not a purchase, and many PCs behind one office address check daily.

/** The bucket's key of the manifest; a release's files are under `<tag>/`. */
export const MANIFEST = 'latest.json';
/** Where the Download button leads until the first release is in the bucket: the releases
 * repository, while it is public. */
export const BEFORE_FIRST = 'https://github.com/nanu1605/vyro-releases/releases/latest';
const TAG = /^v\d{1,4}\.\d{1,4}\.\d{1,4}$/;
const FILE = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;

export function isDownload(path: string): boolean {
  return path === '/download' || path.startsWith('/download/') || path === '/updates/latest.json';
}

function plain(status: number, text: string, extra: Record<string, string> = {}): Response {
  return new Response(text, {
    status,
    headers: { 'Content-Type': 'text/plain; charset=utf-8', 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff', ...extra },
  });
}

/** The installer's path (`/download/<tag>/<file>`) the manifest names, if it names one here. */
export function installerPath(manifest: unknown): string | null {
  const m = manifest as { platforms?: Record<string, { url?: unknown }> } | null;
  const url = m?.platforms?.['windows-x86_64']?.url;
  if (typeof url !== 'string') return null;
  let path: string;
  try {
    path = new URL(url).pathname;
  } catch {
    return null;
  }
  const parts = /^\/download\/([^/]+)\/([^/]+)$/.exec(path);
  return parts && TAG.test(parts[1]!) && FILE.test(parts[2]!) ? path : null;
}

function contentType(file: string): string {
  if (file.endsWith('.msi')) return 'application/x-msi';
  if (file.endsWith('.json')) return 'application/json';
  if (file.endsWith('.txt')) return 'text/plain; charset=utf-8';
  return 'application/octet-stream';
}

/** `Content-Range` for the part of an object of `size` bytes R2 gave. */
function contentRange(r: R2Range, size: number): string {
  let start: number;
  let end: number;
  if ('suffix' in r && r.suffix !== undefined) {
    start = Math.max(0, size - r.suffix);
    end = size - 1;
  } else {
    const o = r as { offset?: number; length?: number };
    start = o.offset ?? 0;
    end = o.length !== undefined ? Math.min(size, start + o.length) - 1 : size - 1;
  }
  return `bytes ${start}-${end}/${size}`;
}

/** Answers a request for a path [`isDownload`] takes. */
export async function download(req: Request, bucket: R2Bucket | undefined): Promise<Response> {
  const path = new URL(req.url).pathname;
  if (req.method !== 'GET' && req.method !== 'HEAD') return plain(405, 'Not allowed', { Allow: 'GET, HEAD' });

  if (path === '/updates/latest.json' || path === '/download') {
    const m = bucket ? await bucket.get(MANIFEST) : null;
    if (!m && path === '/download') return new Response(null, { status: 302, headers: { Location: BEFORE_FIRST, 'Cache-Control': 'no-store' } });
    if (!m) return plain(404, 'No release yet.');
    const text = await m.text();
    if (path === '/updates/latest.json') {
      // Read whole each time: a copy must never be told of an older release than the newest.
      return new Response(req.method === 'HEAD' ? null : text, {
        headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' },
      });
    }
    let to: string | null = null;
    try {
      to = installerPath(JSON.parse(text));
    } catch {
      to = null;
    }
    if (!to) return plain(404, 'No release yet.');
    // Here, on whichever address was asked: the file is on every one.
    return new Response(null, { status: 302, headers: { Location: new URL(to, req.url).toString(), 'Cache-Control': 'no-store' } });
  }

  if (!bucket) return plain(404, 'Not found.');
  const parts = /^\/download\/([^/]+)\/([^/]+)$/.exec(path);
  if (!parts || !TAG.test(parts[1]!) || !FILE.test(parts[2]!)) return plain(404, 'Not found.');
  const [, tag, file] = parts as unknown as [string, string, string];
  const key = `${tag}/${file}`;
  const headers = new Headers({
    'Content-Type': contentType(file),
    // A release's files never change once published (a tag is never made twice).
    'Cache-Control': 'public, max-age=86400',
    'X-Content-Type-Options': 'nosniff',
    'Accept-Ranges': 'bytes',
  });
  if (file.endsWith('.msi')) headers.set('Content-Disposition', `attachment; filename="${file}"`);

  if (req.method === 'HEAD') {
    const o = await bucket.head(key);
    if (!o) return plain(404, 'Not found.');
    headers.set('ETag', o.httpEtag);
    headers.set('Content-Length', String(o.size));
    return new Response(null, { headers });
  }

  const o = await bucket.get(key, { onlyIf: req.headers, range: req.headers });
  if (!o) return plain(404, 'Not found.');
  headers.set('ETag', o.httpEtag);
  if (!('body' in o)) {
    // A condition did not hold: unchanged since the copy the browser has, or changed under it.
    const cached = req.headers.has('If-None-Match') || req.headers.has('If-Modified-Since');
    return new Response(null, { status: cached ? 304 : 412, headers });
  }
  if (o.range && req.headers.has('Range')) {
    headers.set('Content-Range', contentRange(o.range, o.size));
    return new Response(o.body, { status: 206, headers });
  }
  return new Response(o.body, { headers });
}
