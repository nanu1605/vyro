// Short messages after something was done (1.7): the result of an action, said once, over the
// tabs, gone after a few seconds.

export type ToastTone = 'ok' | 'warn' | 'hot';

export interface Toast {
  id: number;
  text: string;
  tone: ToastTone;
}

export const toasts = $state<{ list: Toast[] }>({ list: [] });

let seq = 0;

/** Shows `text`; a problem stays longer. At most three show at once. */
export function toast(text: string, tone: ToastTone = 'ok'): void {
  const id = ++seq;
  toasts.list = [...toasts.list.slice(-2), { id, text, tone }];
  setTimeout(() => dismiss(id), tone === 'ok' ? 4000 : 7000);
}

export function dismiss(id: number): void {
  toasts.list = toasts.list.filter((t) => t.id !== id);
}
