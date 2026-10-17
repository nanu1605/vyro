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
  await step('get_hardware_id');
  await step('get_system_info');
  await step('get_sensor_tree');
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
  await step('set_widgets', { widgets: { version: 1, shown: true, widgets: [...others, widget] } });
  await invoke('smoke_report', { steps });
})();
