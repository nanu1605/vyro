# Fan and pump control: safety matrix

Status: **P27 audit (2026-10-16)**. This lists every rule from V2_PLAN's "Hardware-control safety"
and how each one is met:
- the code that enforces it;
- the automated tests that check it;
- the result on the only writable board, the MSI B650M GAMING PLUS WIFI (MS-7E24, NCT6687D,
  decision 24).

A board is added to the allow-list only after it gets its own copy of the field column.

Paths are under `crates/vyro-core/src/`. Unless another file is named, tests are in the file
named in the "Enforced in" column. "P12" field results are recorded in the ROADMAP's P12
section.

## Who may write at all

| Rule | Enforced in | Tests | MS-7E24 field result |
|---|---|---|---|
| **Read-only by default.** Control is off until the owner turns it on and confirms. | `fan::FanConfig::default` (`enabled: false`); the Cooling tab's confirmation | `nothing_is_driven_when_off_or_on_bios` (`fan/mod.rs`) | Not recorded as its own step; every P12 check started by turning control on |
| **Positive detection only.** A board must match the allow-list by maker, board, chip and revision; anything else is read-only with a reason. | `sio::control::capability`, `fan::win::open_device` | `only_the_reference_board_and_revision_are_writable` (`sio/control.rs`) | P12a: chip 0xD5 rev 0x92 at 0xA20 detected and allow-listed |
| **Only the service writes.** The app's own engine (no service) never drives fans. | `engine::LocalEngine::start_fans` (`fan_control: false` in the app) | `invalid_configs_are_refused_and_a_bad_file_means_off` (an unavailable controller refuses settings) | Covered by the tests |
| **Known registers only.** Writes go to the mode, request and duty registers of allow-listed headers through PawnIO's `LpcIO` module. There are no raw writes, and a busy EC is never forced. | `sio::control::{write_ec, set_duty, release, restore}` | `write_ec_is_one_released_transaction`, `a_busy_ec_is_never_forced`, `set_duty_takes_one_header_and_follows_the_handshake`, `pump_and_unlisted_headers_are_protected` | P12a: `vyro-fans --try 0 100` and `--try 0 30` changed header 0 only |
| **Every write is read back.** | `sio::control::write_checked` | `a_register_that_does_not_keep_the_write_is_reported_and_the_request_closed` | P12a: readback 255 at 100 %, 77 at 30 % |
| **Pump headers need confirmation.** A pump is driven only after the owner confirms it, and it never goes below 60 %. | `fan::FanConfig::{may_drive, floor}`, `sio::control::duty` | `confirmed_pumps_are_driven_never_below_the_pump_floor`, `fixed_speeds_respect_the_floor_and_pumps_are_skipped`, `duty_is_bounded_by_the_header_floor` | P12c: Quiet 60 % → about 3,300 RPM and held; Full → about 4,400 RPM |
| **Settings are checked before use.** Bad limits, points, headers or names are refused. A saved file that fails the check means control off, and the file is kept as `fan-control.bad.json` (P27). | `fan::validate`, `fan::controller::load_config` | `validation_guards_every_field`, `invalid_configs_are_refused_and_a_bad_file_means_off`, `header_names_are_checked` | — |

## What forces safe cooling while driving

| Rule | Enforced in | Tests | MS-7E24 field result |
|---|---|---|---|
| **Floors.** Fans stay at or above 20 % and pumps at or above 60 %. A request Vyro cannot place gets 100 %. | `sio::control::duty`, `fan::Runtime::decide` | `duty_is_bounded_by_the_header_floor`, `fixed_speeds_respect_the_floor_and_pumps_are_skipped` | Covered by the tests (30 % was the lowest tried on hardware) |
| **Emergency limit.** CPU or GPU at its limit → every driven header at 100 %. | `fan::Runtime::decide` | `emergency_limits_force_full_speed_everywhere` | P12b: limit lowered to 50 °C under load → every driven fan at 100 % |
| **A curve whose sensor stops reading** → that header at 100 %. | `fan::Runtime::decide` | `a_missing_source_runs_that_header_at_full_speed` | Covered by the tests (not forced on hardware) |
| **A pump that stops** (below 300 RPM on 3 ticks after spinning) → control ends, and the BIOS gets every header. | `fan::Runtime::decide`, `controller::Worker::fail` | `a_pump_that_stops_reporting_a_speed_ends_control`, `a_stopped_pump_hands_everything_back` | Covered by the tests (not stalled on hardware) |
| **An alert's fan boost** (P18) raises only headers Vyro drives, only while control is on, and never past 100 %. | `fan::Runtime::decide`, `FanControl::set_boost` | `an_alert_boost_raises_driven_headers_only`, `an_alert_boost_is_written_recorded_and_ends` | **Pending** the P18 field check |

## What hands cooling back to the BIOS

| Rule | Enforced in | Tests | MS-7E24 field result |
|---|---|---|---|
| **The BIOS state is saved before the first write.** It is flushed to disk through a temporary file (P27). | `controller::Worker::apply`, `sio::control::save`, `files::write_atomic` | `saves_bios_state_first_writes_on_change_and_restores_when_turned_off`, `bios_state_file_round_trips` | P12b: the crash check below depends on it |
| **Off, Return to BIOS, or one header set to BIOS** puts the saved state back: all of it, or only that header. | `controller::Worker::{restore_all, apply}`, `sio::control::{restore, release}` | `switching_one_header_to_bios_releases_only_it`, `release_hands_back_one_header_only`, `capture_then_restore_returns_every_register` | P12b: Return to BIOS → mode 0x00 |
| **The mode register goes back even if a duty write fails.** | `sio::control::restore` | `restore_still_hands_back_control_when_a_duty_write_fails` | — |
| **Faults end control.** A readback mismatch or 3 I/O errors in a row restore the BIOS state and turn control off. | `controller::Worker::{apply, fail}` | `a_readback_mismatch_restores_and_turns_control_off`, `repeated_io_errors_turn_control_off_after_three`, `port_failure_mid_sequence_is_an_error_not_a_panic` | Covered by the tests |
| **Watchdog.** No sensor tick for 10 s → BIOS until ticks resume. | `controller::Worker::wait` (`WATCHDOG`) | `missing_ticks_trip_the_watchdog_and_control_resumes_with_them` | Covered by the tests |
| **Another program fighting over the fans.** A lost setting is written again; 3 rewrites in 2 minutes end control. | `controller::Worker` (drift reads) | `a_setting_the_chip_lost_is_written_again`, `another_program_fighting_over_the_fans_ends_control` | Not run (MSI Center), covered by the tests |
| **Sleep → BIOS; after waking, the state is captured afresh.** | `controller::FanControl::set_suspended`, the service's power events | `sleep_hands_back_to_the_bios_and_waking_captures_afresh` | P12d: sleep and wake with control on |
| **Service stop, shutdown or uninstall → BIOS.** | `controller::Worker::run` (`Step::Stop`), the MSI's service stop | `shutdown_hands_back_to_the_bios` | P12d: reboot and uninstall with control on |
| **Crash.** At the next start the saved state is written back before anything else (the MSI restarts the service). | `controller::recover` | `a_state_left_by_a_crash_is_restored_before_anything_else` | P12b: killing `vyro-service.exe` brought the BIOS curve back |
| **Crash, then the write-back fails (P27 fix).** The saved state is kept and used for every later attempt. It is never replaced by a capture of the chip as Vyro left it. | `controller::recover` (`Recovery::Failed`), `FanControl::start_with` | `a_failed_restore_at_start_keeps_the_saved_state_for_later_attempts` | **Pending** the P27 field check |
| **Crash, then the saved state cannot be read (P27 fix).** The allow-listed headers under software control go to 100 %. The file is kept as `fan-restore.bad.json`, and the Cooling log says to restart the PC. | `controller::recover` (`Recovery::Unreadable`), `full_speed_where_driven` | `an_unreadable_saved_state_sets_driven_fans_to_full_speed_once` | **Pending** the P27 field check |
| **…and the chip does not answer at start (P27 fix).** Nothing captures the chip or drives a fan until it answers; then full speed comes first. The file stays until then, so a restart meanwhile tries again. | `controller::recover` (`Recovery::Unanswered`), `Worker::full_speed_after_crash` | `a_silent_chip_after_an_unreadable_saved_state_gets_full_speed_before_anything_else` | Covered by the tests (a silent chip cannot be forced on hardware) |

## What the owner sees

| Rule | Enforced in | Tests | MS-7E24 field result |
|---|---|---|---|
| **An always-visible indicator** while control is on: the header chip, the tray tooltip, and "Return fans to BIOS" in the tray menu. | `fan::FanBrief` in every snapshot; the app's header and tray | `the_tooltip_names_fan_control_and_alerts` (`src-tauri/src/tray.rs`) | P12e |
| **A control-event log** of every on and off, full speed, fault, watchdog, recovery, rewrite, sleep, wake, profile and boost. It is kept 7 days or 1,000 entries. | `controller::Inner::record`, the history store's `fan` events | `events_are_logged_and_handed_to_the_sink` | P12e: events kept after a restart |

## Boards

| Board | Chip | Status |
|---|---|---|
| MSI B650M GAMING PLUS WIFI (MS-7E24) | NCT6687D rev 0x92 | **Writable.** Field-checked in P12, plus the pending P18 and P27 checks above |
| Any other board, or another revision of the chip | — | **Read-only**, with the reason shown in the Cooling tab |

To add a board:
1. Make a read-only dump with `vyro-fans` and get a supervised `--try` on each header.
2. Add an allow-list entry with that board's headers and floors.
3. Run the whole field column of this matrix on that board.
