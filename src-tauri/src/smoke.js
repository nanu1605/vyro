// CI smoke test (src-tauri/src/smoke.rs): injected into the main window once its page has
// loaded, only when VYRO_SMOKE is set. Calls what the pages call, through the real IPC, then
// saves one widget as the Widgets page does and hands the steps to `smoke_report`.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const steps = [];
  const timed = (ms, p) =>
    Promise.race([p, new Promise((_, no) => setTimeout(() => no(new Error(`timed out after ${ms / 1000} s`)), ms))]);
  async function step(name, args) {
    const t = performance.now();
    try {
      const value = await timed(20000, invoke(name, args));
      steps.push({ name, ok: true, ms: Math.round(performance.now() - t) });
      return value;
    } catch (e) {
      steps.push({ name, ok: false, ms: Math.round(performance.now() - t), error: String(e?.message ?? e) });
      return null;
    }
  }
  // A call that must be refused, with this reason, in time.
  async function refused(name, args, reason) {
    const t = performance.now();
    const ms = () => Math.round(performance.now() - t);
    try {
      await timed(20000, invoke(name, args));
      steps.push({ name, ok: false, ms: ms(), error: 'was not refused' });
    } catch (e) {
      const error = String(e?.message ?? e);
      steps.push(error.includes(reason) ? { name, ok: true, ms: ms() } : { name, ok: false, ms: ms(), error });
    }
  }
  // Counts an event's deliveries to this page, as the pages' own listeners get them; the
  // function it gives stops counting (after a second for late ones) and answers the count.
  async function hear(event) {
    let count = 0;
    const handler = window.__TAURI_INTERNALS__.transformCallback(() => count++);
    const eventId = await timed(20000, invoke('plugin:event|listen', { event, target: { kind: 'Any' }, handler }));
    return async () => {
      await new Promise((r) => setTimeout(r, 1000));
      await invoke('plugin:event|unlisten', { event, eventId }).catch(() => {});
      return count;
    };
  }
  // A read that must not announce `event`: a page that reads again when it hears it would read
  // for as long as it shows (Settings → Remote did, field check of 1.7). `canHear`: this page
  // heard an event it should have, so silence means something.
  async function quiet(name, event, canHear) {
    const t = performance.now();
    const ms = () => Math.round(performance.now() - t);
    try {
      const stop = await hear(event);
      await timed(20000, invoke(name));
      const n = await stop();
      if (n) steps.push({ name, ok: false, ms: ms(), error: `announced ${event} ${n} time(s)` });
      else if (!canHear) steps.push({ name, ok: false, ms: ms(), error: 'cannot tell: saving widgets was not heard either' });
      else steps.push({ name, ok: true, ms: ms() });
    } catch (e) {
      steps.push({ name, ok: false, ms: ms(), error: String(e?.message ?? e) });
    }
  }
  await step('get_hardware_id');
  await step('get_system_info');
  await step('get_sensor_tree');
  await step('get_devices');
  await refused(
    'set_device_connected',
    { id: '0a1b2c3d4e5f', control: 'link', connect: false },
    'Not a paired Bluetooth device',
  );
  const current = await step('get_widgets');
  const widget = {
    id: 'smoke1',
    kind: 'gauge',
    sources: ['cpu.usage'],
    x: 0,
    y: 0,
    placed: false,
    scale: 100,
    opacity: 85,
    style: 'app',
    layer: 'desktop',
    locked: false,
    clickThrough: false,
  };
  const others = (current?.widgets ?? []).filter((w) => w.id !== widget.id).slice(0, 7);
  const saved = await hear('widgets-changed').catch(() => null);
  await step('set_widgets', { widgets: { version: 1, shown: true, widgets: [...others, widget] } });
  const heard = saved ? await saved() : 0;
  await quiet('get_remote_status', 'remote-status', heard > 0);
  await invoke('smoke_report', { steps });
})();
