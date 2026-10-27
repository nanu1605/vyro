// What's new (1.5): each version's notes, newest first. The app shows them once after Vyro was
// updated (src-tauri/src/whats_new.rs decides when); the phone app shows the ones marked
// `phone` after it changes, up to the open PC's version. Every version gets an entry before it
// is released (a test checks package.json's version has one).

/** One of the icons the dialogs draw (WhatsNew.svelte). */
export type NoteIcon = 'phone' | 'end' | 'power' | 'wake' | 'fan' | 'gauge' | 'gpu' | 'screen' | 'mic' | 'sparkles';

export interface NoteItem {
  icon: NoteIcon;
  title: string;
  text: string;
  /** Shown by the phone app too. */
  phone?: boolean;
}

export interface Release {
  version: string;
  items: NoteItem[];
}

export const NOTES: Release[] = [
  {
    version: '2.0.0',
    items: [
      {
        icon: 'phone',
        title: 'Notifications on your phone',
        text: 'Turn them on in More → Notifications on your phone: alerts, a low mouse or headset battery, game summaries and this PC going quiet reach you even with the app closed. Each is sealed for your phone on this PC; the relay passes it on and can’t read it.',
        phone: true,
      },
      {
        icon: 'power',
        title: 'Told when this PC stops answering',
        text: 'If this PC loses power, crashes or drops off its network and is not back within two minutes, your phone hears it, and again when it is back. Shutting down, restarting and sleeping say nothing.',
        phone: true,
      },
      {
        icon: 'gpu',
        title: 'GPU tuning for AMD and Intel',
        text: 'Radeon RX 6000 to RX 9000 and Arc A- and B-series cards get the presets, your own settings, the stability test and “Find my best”, through AMD’s and Intel’s own driver interfaces, as NVIDIA cards have since 1.6.',
      },
      {
        icon: 'phone',
        title: 'Which phones get notifications',
        text: 'Settings → Remote says which paired phones get notifications. Each phone turns them on and off for itself.',
      },
    ],
  },
  {
    version: '1.7.8',
    items: [
      {
        icon: 'wake',
        title: 'Wake a PC through another',
        text: '“Wake” on a sleeping PC’s page on your phone now has another of your PCs wake it, rather than always saying none could.',
        phone: true,
      },
      {
        icon: 'screen',
        title: 'Leaving a PC ends its screen session',
        text: 'Going to your list of PCs, opening another PC, scanning a code or signing out while your phone shows this PC’s screen now ends the session at once, not up to a minute later.',
        phone: true,
      },
      {
        icon: 'screen',
        title: 'HD stays HD on mobile data',
        text: 'Moving onto mobile data during a session no longer shows SD on your phone while this PC goes on sending HD: the session keeps its quality until you switch it.',
        phone: true,
      },
      {
        icon: 'phone',
        title: 'Your phone on a slow connection',
        text: 'Processes and More now wait for this PC’s last answer before asking again, and the Screen tab asks again for this PC’s monitors when it could not get them.',
        phone: true,
      },
      {
        icon: 'screen',
        title: 'A new relay address ends a phone session',
        text: 'Changing the relay address in Settings → Remote signs every phone out, and now ends a phone’s session with this PC’s screen too.',
      },
      {
        icon: 'phone',
        title: 'A phone’s session ends with its pairing',
        text: 'A phone that signs itself out of this PC, or whose pairing runs out, now has its session with this PC’s screen ended at once.',
      },
    ],
  },
  {
    version: '1.7.7',
    items: [
      {
        icon: 'screen',
        title: 'Your screen stays on your phone through a dropped connection',
        text: 'The phone now keeps showing this PC’s screen when its connection drops for a moment, as when it changes networks, and the slow view reconnects rather than ending.',
        phone: true,
      },
      {
        icon: 'screen',
        title: 'Another monitor without locking',
        text: 'With “Lock this PC when a phone’s session ends” on, choosing another monitor on your phone, or its falling back to the slow view, no longer locks this PC in between.',
      },
      {
        icon: 'mic',
        title: 'Your phone’s microphone ends with its session',
        text: 'A session’s end no longer leaves your phone’s microphone as Windows’ default for a few seconds after, and Vyro puts yours back as it closes.',
      },
      {
        icon: 'mic',
        title: 'A voice clip you cancel stops',
        text: 'Closing the clip sheet while a clip sends, or tapping its new Cancel, now stops sending it, so it no longer lands on this PC later.',
        phone: true,
      },
      {
        icon: 'phone',
        title: 'Pages that keep what they showed',
        text: 'One refresh that fails no longer empties a page on your phone: Health, Processes, Devices and More keep what this PC said last.',
        phone: true,
      },
    ],
  },
  {
    version: '1.7.6',
    items: [
      {
        icon: 'mic',
        title: 'Your phone’s microphone stays the one Windows hears',
        text: 'Turning your phone’s microphone off and on again just as it starts no longer leaves Windows’ apps hearing this PC’s own microphone instead of your phone’s.',
      },
      {
        icon: 'screen',
        title: 'Updates end a phone session first',
        text: 'Installing an update while your phone shows this PC’s screen now lets go of keys the phone held, and puts your default microphone back, before Vyro closes for the installer.',
      },
    ],
  },
  {
    version: '1.7.5',
    items: [
      {
        icon: 'screen',
        title: 'Minutes left that count down',
        text: 'While your phone shows this PC’s screen through the slow view, Settings → Remote now counts its minutes left for today down as they go, not only when Settings opens.',
      },
      {
        icon: 'screen',
        title: 'Text with a hidden character still types',
        text: 'Text typed from your phone that holds an invisible control character, as some pasted text does, now types without it here, rather than not at all.',
      },
    ],
  },
  {
    version: '1.7.4',
    items: [
      {
        icon: 'screen',
        title: 'Taps that follow a new resolution',
        text: 'When this PC’s resolution changes while your phone shows its screen (set here, or by a game), taps from the phone now land where they are shown, not where the screen used to be.',
      },
    ],
  },
  {
    version: '1.7.3',
    items: [
      {
        icon: 'screen',
        title: 'Keys from your phone let go when a session ends',
        text: 'Ctrl, Alt, Shift or Win tapped on your phone, or a drag, no longer stays held down on this PC when its screen session ends first: whoever uses the PC next types letters, not shortcuts.',
      },
    ],
  },
  {
    version: '1.7.2',
    items: [
      {
        icon: 'gpu',
        title: 'Back to stock when the driver refuses a setting',
        text: 'If the driver takes some of a tuning’s settings but refuses the next, the card now goes back to stock, rather than keeping those it took while Vyro showed Stock.',
      },
      {
        icon: 'phone',
        title: 'Updates from your phone ask first',
        text: 'Someone at this PC must allow an update, and Vyro closes here while it installs, so your phone now asks before it starts one. While this PC is locked, install it here.',
      },
    ],
  },
  {
    version: '1.7.1',
    items: [
      {
        icon: 'sparkles',
        title: 'What 1.5 and 1.6 brought',
        text: 'After an update from Vyro 1.4, 1.7.0 showed only its own news. What’s new now shows every version’s, and Settings → Updates → What’s new lists them all.',
      },
      {
        icon: 'mic',
        title: 'Voice clips that say when the connection drops',
        text: 'A clip sent from your phone as the connection is lost now says so, rather than “Sending…” for good.',
      },
    ],
  },
  {
    version: '1.7.0',
    items: [
      {
        icon: 'screen',
        title: 'This PC’s screen on your phone',
        text: 'See it and use it from anywhere: tap to click, hold for a right click, two fingers to scroll, pinch to zoom, and type. Allow it per phone in Settings → Remote.',
        phone: true,
      },
      {
        icon: 'sparkles',
        title: 'Always shows something',
        text: 'Where no direct connection can be made, the phone shows a picture at a time through the relay instead.',
        phone: true,
      },
      {
        icon: 'phone',
        title: 'A new phone app',
        text: 'Tabs at the bottom within your thumb’s reach, sheets that rise from below, and a little motion throughout. Everything fits your phone’s width.',
        phone: true,
      },
      {
        icon: 'screen',
        title: 'Full screen',
        text: 'The PC’s screen fills your phone, turned sideways for a wide screen where the phone allows. Its controls step aside until you need them.',
        phone: true,
      },
      {
        icon: 'gauge',
        title: 'HD or SD',
        text: 'On mobile data the phone asks which you’d like: SD uses about a fifth of the data. Switch any time from the toolbar.',
        phone: true,
      },
      {
        icon: 'mic',
        title: 'Voice clips for your chats',
        text: 'Record a clip on your phone: it lands on this PC’s clipboard as a file, ready to paste into WhatsApp Web or any chat.',
        phone: true,
      },
      {
        icon: 'mic',
        title: 'Your phone as this PC’s microphone',
        text: 'With the free VB-CABLE installed here, the phone’s Mic button lets apps on this PC hear you, WhatsApp’s voice notes too. Settings → Remote says more.',
        phone: true,
      },
    ],
  },
  {
    version: '1.6.0',
    items: [
      {
        icon: 'gpu',
        title: 'GPU tuning for NVIDIA cards',
        text: 'Presets made for your card (Quiet, Efficient, Performance) and your own settings, within the limits the driver allows. Open GPU tuning in the Performance group.',
      },
      {
        icon: 'gauge',
        title: 'Test before you trust it',
        text: 'A 5-minute stability test checks a setting, and Find my best works out the most your card keeps up with.',
      },
      {
        icon: 'sparkles',
        title: 'Back to stock whenever you want',
        text: 'Too hot, a crash or another tuning program puts the card back to stock by itself. The top bar and the tray always offer Stock.',
      },
      {
        icon: 'phone',
        title: 'Switch presets from your phone',
        text: 'A phone allowed to change this PC can pick Stock or a tested preset.',
        phone: true,
      },
    ],
  },
  {
    version: '1.5.0',
    items: [
      {
        icon: 'phone',
        title: 'Control this PC from your phone',
        text: 'On a phone you allow in Settings → Remote: change the profile and fans, turn alerts on or off, run a benchmark or diagnostics, and connect devices.',
        phone: true,
      },
      {
        icon: 'end',
        title: 'End a task',
        text: 'End a program that has stopped responding, from Processes here or from your phone.',
        phone: true,
      },
      {
        icon: 'power',
        title: 'Shut down, restart, sleep or lock from your phone',
        text: 'A shutdown or restart counts down here first, with Cancel.',
        phone: true,
      },
      {
        icon: 'wake',
        title: 'Wake a PC from your phone',
        text: 'Another Vyro PC on the same network sends the wake signal. Turn on Wake from phone in Settings → Remote.',
        phone: true,
      },
      {
        icon: 'fan',
        title: 'Fans you can see',
        text: 'My PC’s case is open where its fans are, so they show from outside.',
      },
      {
        icon: 'gauge',
        title: 'CPU use as Task Manager shows it',
        text: 'And the gauges follow it more closely.',
      },
    ],
  },
];

/** -1, 0 or 1; a version that is not `major.minor.patch` counts as 0.0.0. */
export function compareVersions(a: string, b: string): number {
  const parts = (v: string) => {
    const core = v.trim().split(/[-+]/)[0] ?? '';
    const n = core.split('.').map((x) => Number(x));
    return n.length <= 3 && n.every((x) => Number.isInteger(x) && x >= 0) ? [n[0] ?? 0, n[1] ?? 0, n[2] ?? 0] : [0, 0, 0];
  };
  const [x, y] = [parts(a), parts(b)];
  for (let i = 0; i < 3; i++) {
    const d = (x[i] ?? 0) - (y[i] ?? 0);
    if (d) return Math.sign(d);
  }
  return 0;
}

/** The notes to show after an update from `from` to `current`, newest first. `from` null: a
 * Vyro before 1.5, which kept no version, so every version's notes up to `current` (they begin
 * at 1.5, and 1.5 and 1.6 were released only within 1.7). */
export function notesSince(from: string | null, current: string, notes: Release[] = NOTES): Release[] {
  const upTo = notes.filter((r) => compareVersions(r.version, current) <= 0);
  if (from === null) return upTo;
  return upTo.filter((r) => compareVersions(r.version, from) > 0);
}

/** The phone's notes (1.7) for a PC whose Vyro is `pc`, after those it showed up to `seen`:
 * only up to the PC's version, since the phone app is newer than a PC not yet updated (the relay
 * serves it to every phone) and what it brings needs the PC's Vyro too. `upTo`: the version to
 * remember once shown, so the rest shows when the PC is updated. */
export function phoneNotes(seen: string | null, pc: string, notes: Release[] = NOTES): { upTo: string; items: NoteItem[] } {
  const newest = notes[0]?.version ?? '';
  const upTo = compareVersions(pc, newest) < 0 ? pc : newest;
  return { upTo, items: notesSince(seen, upTo, notes).flatMap((r) => r.items.filter((i) => i.phone)) };
}

/** "1.5.0" → "1.5"; a patch version stays whole ("1.5.1"). */
export function shortVersion(v: string): string {
  return v.replace(/^(\d+\.\d+)\.0$/, '$1');
}
