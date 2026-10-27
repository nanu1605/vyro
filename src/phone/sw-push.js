// Notifications with the app closed (2.0), in the phone app's service worker (vite.phone.config.ts
// adds this to it). A PC sealed each for this browser; the browser opened it, and it is shown
// here. A tap opens the app on that PC. Plain JavaScript: the service worker is a script of its
// own, outside the app's build.

/** What a push says, as `showNotification` takes it. A push that does not read still shows
 * something: a browser takes notifications away from a site whose pushes show nothing. */
function vyroNote(text) {
  let n = null;
  try {
    n = JSON.parse(text);
  } catch {
    /* shown plainly below */
  }
  const str = (v, max) => (typeof v === 'string' ? v.slice(0, max) : '');
  const ok = n !== null && typeof n === 'object' && n.v === 1;
  const pc = ok && /^[0-9a-f]{32}$/.test(n.pc) ? n.pc : null;
  const tag = ok ? str(n.tag, 64) : '';
  return {
    title: (ok && str(n.title, 120)) || 'Vyro',
    options: {
      body: ok ? str(n.body, 300) : 'Open Vyro to see what your PC said.',
      // A newer one about the same thing (a battery, the PC going quiet) replaces the older.
      tag: pc && tag ? `${pc}:${tag}` : undefined,
      renotify: Boolean(pc && tag),
      // Seconds from the PC; 0 (the relay's "stopped answering"): when it arrived.
      timestamp: ok && typeof n.ts === 'number' && n.ts > 0 ? n.ts * 1000 : Date.now(),
      icon: '/icons/icon-192.png',
      badge: '/icons/icon-192.png',
      data: { pc },
    },
  };
}

self.addEventListener('push', (e) => {
  const note = vyroNote(e.data ? e.data.text() : '');
  e.waitUntil(self.registration.showNotification(note.title, note.options));
});

/** The app on PC `pc`: a window already open is brought forward and told; otherwise one opens. */
async function vyroOpen(pc) {
  const url = pc ? `/?pc=${pc}` : '/';
  const open = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
  const win = open.find((c) => new URL(c.url).origin === self.location.origin);
  if (win) {
    await win.focus();
    if (pc) win.postMessage({ t: 'open', pc });
    return;
  }
  await self.clients.openWindow(url);
}

self.addEventListener('notificationclick', (e) => {
  e.notification.close();
  e.waitUntil(vyroOpen(e.notification.data && e.notification.data.pc));
});
