// What's new (1.5): each version's notes, newest first. The app shows them once after Vyro was
// updated (src-tauri/src/whats_new.rs decides when); the phone app shows the ones marked
// `phone` after it changes. Every version gets an entry before it is released (a test checks
// package.json's version has one).

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

/** The notes to show after an update from `from` (null: a Vyro before 1.5, so this version's)
 * to `current`, newest first. */
export function notesSince(from: string | null, current: string, notes: Release[] = NOTES): Release[] {
  const upTo = notes.filter((r) => compareVersions(r.version, current) <= 0);
  if (from === null) return upTo.slice(0, 1);
  return upTo.filter((r) => compareVersions(r.version, from) > 0);
}

/** "1.5.0" → "1.5"; a patch version stays whole ("1.5.1"). */
export function shortVersion(v: string): string {
  return v.replace(/^(\d+\.\d+)\.0$/, '$1');
}
