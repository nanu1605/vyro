//! A phone's requests through the relay (1.5), answered here in the app with the same work the
//! window's commands do. `vyro_core::remote::relay::api` lists them and who may make them; the
//! relay connection has already checked the phone's session and permission. What a phone
//! changed is kept for Settings → Remote, and ending a process or a power action also shows a
//! notification on the PC.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use vyro_core::actions::power::{self, PendingPower, PowerRequest, PowerState};
use vyro_core::actions::{process, wake};
use vyro_core::alerts::AlertRule;
use vyro_core::bench::BenchMode;
use vyro_core::devices::Control;
use vyro_core::engine::EngineApi;
use vyro_core::profiles::Profile;
use vyro_core::remote::relay::api::{self, Access, PhoneAction, MAX_ACTIONS};
use vyro_core::remote::relay::{Reply, Request};
use vyro_core::{files, lock, unix_now};

use crate::engine::Engine;

/// Emitted with the [`PowerState`] when a shutdown or restart is asked or cancelled.
pub const POWER_EVENT: &str = "power-pending";
/// A phone asked to install an update while the PC is locked.
const UPDATE_LOCKED: &str =
    "The PC is locked, and Windows asks someone at it to allow the update: install it there.";
/// Emitted (no payload) when a phone changed something.
pub const ACTIONS_EVENT: &str = "phone-actions";
const ACTIONS_FILE: &str = "phone-actions.json";

/// What phones changed (newest last), and a shutdown or restart under way.
#[derive(Default)]
pub struct PhoneState {
    path: Option<PathBuf>,
    actions: Mutex<VecDeque<PhoneAction>>,
    pending: Mutex<Option<PendingPower>>,
}

impl PhoneState {
    pub fn load(app: &AppHandle) -> Self {
        let path = app
            .path()
            .app_local_data_dir()
            .ok()
            .map(|d| d.join(ACTIONS_FILE));
        let actions = path
            .as_deref()
            .and_then(|p| files::read_json::<VecDeque<PhoneAction>>(p, |_| Ok(())).ok())
            .flatten()
            .unwrap_or_default();
        Self {
            path,
            actions: Mutex::new(actions),
            pending: Mutex::new(None),
        }
    }

    fn record(&self, action: PhoneAction) {
        let mut list = lock(&self.actions);
        list.push_back(action);
        while list.len() > MAX_ACTIONS {
            list.pop_front();
        }
        if let Some(p) = &self.path {
            if let Err(e) = files::write_json(p, &*list) {
                log::warn!("phones: {e}");
            }
        }
    }

    /// A shutdown or restart still waiting (one whose time passed is forgotten).
    fn pending(&self) -> Option<PendingPower> {
        let mut p = lock(&self.pending);
        if p.as_ref().is_some_and(|x| x.at < unix_now()) {
            *p = None;
        }
        p.clone()
    }
}

/// Answers a request on its own thread: some take seconds (Connect, diagnostics).
pub fn handle(app: &AppHandle, r: Request, reply: Reply) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("vyro-phone".into())
        .spawn(move || {
            let result = call(&app, &r);
            if api::access(&r.method) == Some(Access::Control) {
                record(&app, &r, &result);
            }
            if let Err(e) = &result {
                log::info!("phones: {} from {}: {e}", r.method, r.name);
            }
            reply.send(result);
        });
    if let Err(e) = spawned {
        log::warn!("phones: cannot answer a request: {e}");
    }
}

fn record(app: &AppHandle, r: &Request, result: &Result<Value, String>) {
    keep(
        app,
        PhoneAction {
            at: unix_now(),
            phone: r.name.clone(),
            what: api::describe(&r.method, &r.params),
            error: result.as_ref().err().cloned(),
        },
    );
}

/// Something a phone did that is not one request (a screen session's start and end).
pub fn log_action(app: &AppHandle, phone: &str, what: String) {
    keep(
        app,
        PhoneAction {
            at: unix_now(),
            phone: phone.to_owned(),
            what,
            error: None,
        },
    );
}

fn keep(app: &AppHandle, action: PhoneAction) {
    let Some(state) = app.try_state::<PhoneState>() else {
        return;
    };
    state.record(action);
    if let Err(e) = app.emit(ACTIONS_EVENT, ()) {
        log::debug!("emit {ACTIONS_EVENT}: {e}");
    }
}

fn param<T: DeserializeOwned>(p: &Value, key: &str) -> Result<T, String> {
    p.get(key)
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .ok_or_else(|| format!("The request needs \"{key}\"."))
}

fn to_json(v: impl Serialize) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

fn done() -> Result<Value, String> {
    Ok(json!({ "done": true }))
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        log::warn!("phones: notification failed: {e}");
    }
}

fn call(app: &AppHandle, r: &Request) -> Result<Value, String> {
    // The screen (1.7) keeps its own record of sessions.
    if api::access(&r.method) == Some(Access::Screen) {
        return crate::screen::handle(app, r);
    }
    let engine = app.state::<Engine>();
    let e: &dyn EngineApi = engine.api.as_ref();
    let p = &r.params;
    match r.method.as_str() {
        // Reading.
        "processes" => to_json(e.latest().map(|s| s.processes).unwrap_or_default()),
        "devices" => {
            let devices = crate::commands::collect_devices();
            crate::device_watch::store(app, &devices);
            to_json(devices)
        }
        "health" => to_json(e.latest().map(|s| s.health).unwrap_or_default()),
        "insights" => to_json(e.insights()?),
        "score" => to_json(e.performance_score()?),
        "alerts" => to_json(json!({ "rules": e.alert_rules()?, "log": e.alert_log()? })),
        "fans" => to_json(e.fan_status()?),
        "profiles" => to_json(json!({
            "list": Profile::ALL.iter().map(|p| p.spec()).collect::<Vec<_>>(),
            "state": crate::gaming::effective(app),
        })),
        "bench.runs" => to_json(e.benchmark_runs()?),
        "bench.run" => to_json(e.benchmark_run(param(p, "id")?)?),
        "bench.progress" => {
            let state = app.state::<crate::bench::BenchState>();
            to_json(json!({
                "progress": crate::bench::progress(&state),
                "preflight": crate::bench::preflight_now(app, &state),
            }))
        }
        "diag.runs" => to_json(e.diagnostic_runs()?),
        "diag.run" => to_json(e.diagnostic_run(param(p, "id")?)?),
        "changes" => to_json(e.hardware_changes(20)?),
        "updates" => to_json(app.state::<crate::updates::Updates>().status()),
        "power.state" => to_json(power_state(app)),
        "gpu.tune" => to_json(e.gpu_tune_status()?),

        // Changing.
        "profile.pick" => {
            crate::tray::pick_profile(app, param::<Profile>(p, "profile")?)?;
            done()
        }
        "profile.keep" => {
            crate::gaming::manual_pick(app);
            crate::gaming::apply_effective(app);
            done()
        }
        "fans.bios" => {
            e.restore_fans()?;
            done()
        }
        "alerts.enable" => {
            let id: String = param(p, "id")?;
            let on: bool = param(p, "on")?;
            let mut rules: Vec<AlertRule> = e.alert_rules()?;
            let rule = rules
                .iter_mut()
                .find(|x| x.id == id)
                .ok_or("No such alert.")?;
            rule.enabled = on;
            e.set_alert_rules(rules)?;
            done()
        }
        "bench.start" => {
            crate::bench::start(app, param::<BenchMode>(p, "mode")?)?;
            done()
        }
        "bench.stop" => {
            crate::bench::stop(app, "Stopped from a phone.");
            done()
        }
        "diag.start" => to_json(e.run_diagnostics(crate::diag::app_info(app, &engine))?),
        "device.connect" => to_json(crate::commands::device_action(
            e,
            &param::<String>(p, "id")?,
            param::<Control>(p, "control")?,
            param(p, "connect")?,
        )?),
        "process.end" => {
            let pid: u32 = param(p, "pid")?;
            let name: String = param(p, "name")?;
            process::end(pid, &name)?;
            notify(
                app,
                "Vyro",
                &format!("{} ended {name} from a phone.", r.name),
            );
            done()
        }
        "updates.check" => to_json(tauri::async_runtime::block_on(crate::updates::check(
            app, false,
        ))),
        "updates.install" => {
            // Windows asks someone at the PC to allow the installer, and Vyro closes as it
            // starts: on a locked PC nobody can, and the phone would lose the PC until then.
            if power::locked() == Some(true) {
                return Err(UPDATE_LOCKED.into());
            }
            tauri::async_runtime::block_on(crate::updates::install_update(app.clone()))?;
            done()
        }
        "power" => {
            let req: PowerRequest =
                serde_json::from_value(p.clone()).map_err(|_| "Not a power action Vyro knows.")?;
            power_action(app, &req, &r.name)?;
            to_json(power_state(app))
        }
        "power.cancel" => {
            cancel_power(app)?;
            to_json(power_state(app))
        }
        "gpu.preset" => {
            gpu_preset(e, &param::<String>(p, "id")?)?;
            done()
        }
        "gpu.stock" => {
            crate::gpu_tune::stop(app);
            e.restore_gpu()?;
            done()
        }
        "wake" => {
            let mac =
                wake::parse_mac(&param::<String>(p, "mac")?).ok_or("Not a network address.")?;
            let sent = wake::send(&mac, &wake_sources())?;
            to_json(json!({ "sent": sent }))
        }
        _ => Err(api::UNKNOWN.into()),
    }
}

/// Applies a GPU preset a phone chose, for the profile in force: only Stock or one whose
/// settings passed the stability test on this card (custom values are set at the PC).
fn gpu_preset(e: &dyn EngineApi, id: &str) -> Result<(), String> {
    let status = e.gpu_tune_status()?;
    if !status.config.enabled {
        return Err("GPU tuning is off on the PC.".into());
    }
    let preset = status
        .presets
        .iter()
        .find(|p| p.preset.id == id)
        .ok_or("No such preset.")?;
    if preset.tested_at.is_none() {
        return Err("From a phone, only Stock and presets that passed the stability test.".into());
    }
    let mut cfg = status.config;
    match cfg.profiles.get_mut(&status.profile) {
        Some(own) => id.clone_into(own),
        None => id.clone_into(&mut cfg.preset),
    }
    e.set_gpu_tune(cfg, true)
}

/// This PC's IPv4 addresses with their prefix lengths, to send wake packets from.
pub fn wake_sources() -> Vec<(std::net::Ipv4Addr, u8)> {
    crate::commands::collect_network()
        .iter()
        .filter(|a| a.connected && !a.virtual_adapter)
        .flat_map(|a| a.ipv4.iter())
        .filter_map(|cidr| {
            let (ip, prefix) = cidr.split_once('/').unwrap_or((cidr, "24"));
            Some((ip.parse().ok()?, prefix.parse().ok()?))
        })
        .collect()
}

pub fn power_state(app: &AppHandle) -> PowerState {
    let (can_sleep, can_hibernate) = power::abilities();
    PowerState {
        pending: app.try_state::<PhoneState>().and_then(|s| s.pending()),
        can_sleep,
        can_hibernate,
    }
}

fn emit_power(app: &AppHandle) {
    if let Err(e) = app.emit(POWER_EVENT, power_state(app)) {
        log::debug!("emit {POWER_EVENT}: {e}");
    }
}

/// A power action; a shutdown or restart is shown on the PC until it happens or is cancelled.
fn power_action(app: &AppHandle, req: &PowerRequest, by: &str) -> Result<(), String> {
    let state = app.state::<PhoneState>();
    if req.action.delayed() && state.pending().is_some() {
        return Err("A shutdown or restart is already waiting.".into());
    }
    let wait = req.wait();
    if req.action.delayed() {
        // Shown first: a shutdown "now" may end this process before it returns.
        *lock(&state.pending) = Some(PendingPower {
            action: req.action,
            at: unix_now() + i64::from(wait),
            by: by.to_owned(),
        });
        emit_power(app);
        notify(
            app,
            &format!("{} in {wait} s", req.action.label()),
            &format!("Asked from {by}. Open Vyro from its tray icon to cancel."),
        );
    }
    let result = power::act(req, by);
    if result.is_err() && req.action.delayed() {
        *lock(&state.pending) = None;
        emit_power(app);
    }
    result
}

fn cancel_power(app: &AppHandle) -> Result<(), String> {
    power::cancel()?;
    if let Some(s) = app.try_state::<PhoneState>() {
        *lock(&s.pending) = None;
    }
    emit_power(app);
    Ok(())
}

/// The shutdown or restart a phone asked for, for the window's banner.
#[tauri::command]
pub fn get_power_state(app: AppHandle) -> PowerState {
    power_state(&app)
}

/// Cancels it from the window.
#[tauri::command]
pub async fn cancel_power_action(app: AppHandle) -> Result<PowerState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        cancel_power(&app)?;
        Ok(power_state(&app))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What phones changed, newest first.
#[tauri::command]
pub fn get_phone_actions(state: tauri::State<'_, PhoneState>) -> Vec<PhoneAction> {
    lock(&state.actions).iter().rev().cloned().collect()
}
