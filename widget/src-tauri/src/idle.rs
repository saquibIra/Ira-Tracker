//! W6/W7: idle, screen-lock and sleep detection with the Keep / Discard / Assign prompt.
//!
//! Privacy: the only signal read is the OS counter "seconds since the last input event";
//! no keystrokes, mouse positions or window information. A locked screen counts as idle
//! because no input reaches the session; sleep shows up as a jump in wall-clock time
//! between two checks.

use crate::{AppState, changed, db, fmt_time, refresh_tray};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

pub const IDLE_WINDOW: &str = "idle";
/// Default idle threshold (W6: 10 minutes, configurable).
const DEFAULT_IDLE_MINUTES: i64 = 10;
/// Check interval while a timer runs (matches the 30 s polling budget).
const CHECK_EVERY: Duration = Duration::from_secs(30);
/// Faster checks while the user is away, so the prompt appears soon after return.
const CHECK_AWAY: Duration = Duration::from_secs(3);
/// Input within this many seconds means the user is back.
const BACK_SECS: i64 = 5;

/// An away period waiting for the user's decision.
#[derive(Clone, Serialize)]
pub struct Away {
    pub start: String,
    pub end: String,
    pub minutes: i64,
    pub entry: String,
    pub label: String,
}

/// Threshold in seconds: `IRA_IDLE_SECS` (testing) overrides the `idle_minutes` setting.
fn threshold_secs(st: &AppState) -> i64 {
    if let Some(s) = std::env::var("IRA_IDLE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
    {
        return s;
    }
    db::kv_get(&st.db.lock().unwrap(), "idle_minutes")
        .and_then(|m| m.parse::<i64>().ok())
        .unwrap_or(DEFAULT_IDLE_MINUTES)
        * 60
}

#[cfg(target_os = "macos")]
fn system_idle_secs() -> Option<i64> {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    }
    // kCGEventSourceStateCombinedSessionState = 0, kCGAnyInputEventType = ~0
    let secs = unsafe { CGEventSourceSecondsSinceLastEventType(0, u32::MAX) };
    secs.is_finite().then_some(secs as i64)
}

#[cfg(windows)]
fn system_idle_secs() -> Option<i64> {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return None;
    }
    Some((unsafe { GetTickCount() }.wrapping_sub(info.dwTime) / 1000) as i64)
}

/// Other platforms: only sleep is detected (via the wall-clock jump).
#[cfg(not(any(target_os = "macos", windows)))]
fn system_idle_secs() -> Option<i64> {
    None
}

/// Away-detection state machine. Pure (no I/O) so it can be unit-tested; `monitor`
/// feeds it the clock and the OS idle counter.
#[derive(Default)]
pub struct Detector {
    last_tick: Option<DateTime<Utc>>,
    last_input: Option<DateTime<Utc>>,
    away_since: Option<DateTime<Utc>>,
}

impl Detector {
    pub fn is_away(&self) -> bool {
        self.away_since.is_some()
    }

    /// Called when there is nothing to watch (no timer, or a prompt already open).
    pub fn skip(&mut self, now: DateTime<Utc>) {
        *self = Detector {
            last_tick: Some(now),
            ..Default::default()
        };
    }

    /// One check, `interval` seconds after the previous one. `idle` is the OS
    /// "seconds since last input" (None where unsupported). Returns the away period
    /// `(start, end)` once the user is back, if it lasted at least `threshold` seconds.
    pub fn tick(
        &mut self,
        now: DateTime<Utc>,
        interval: i64,
        idle: Option<i64>,
        threshold: i64,
        timer_start: DateTime<Utc>,
    ) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        let gap = self.last_tick.map_or(interval, |t| (now - t).num_seconds());
        self.last_tick = Some(now);
        if self.away_since.is_none() {
            if gap - interval >= threshold {
                // The machine slept: the process was suspended between checks.
                let before = now - chrono::Duration::seconds(gap);
                self.away_since = Some(self.last_input.unwrap_or(before));
            } else if let Some(i) = idle.filter(|i| *i >= threshold) {
                self.away_since = Some(now - chrono::Duration::seconds(i));
            }
        }
        if let Some(i) = idle {
            self.last_input = Some(now - chrono::Duration::seconds(i));
        }
        let back = idle.is_none_or(|i| i < BACK_SECS);
        let since = self.away_since.filter(|_| back)?;
        self.away_since = None;
        let start = since.max(timer_start);
        let end = now - chrono::Duration::seconds(idle.unwrap_or(0));
        ((end - start).num_seconds() >= threshold).then_some((start, end))
    }
}

/// Background loop: only does work while a timer runs and no prompt is open.
pub async fn monitor(app: AppHandle) {
    let mut det = Detector::default();
    loop {
        let interval = if det.is_away() {
            CHECK_AWAY
        } else {
            CHECK_EVERY
        };
        tokio::time::sleep(interval).await;
        let st = app.state::<AppState>();
        let now = Utc::now();

        let running = db::running(&st.db.lock().unwrap()).ok().flatten();
        let Some(entry) = running.filter(|_| st.away.lock().unwrap().is_none()) else {
            det.skip(now);
            continue;
        };
        let Ok(timer_start) = DateTime::parse_from_rfc3339(&entry.start) else {
            det.skip(now);
            continue;
        };
        let Some((start, end)) = det.tick(
            now,
            interval.as_secs() as i64,
            system_idle_secs(),
            threshold_secs(&st),
            timer_start.with_timezone(&Utc),
        ) else {
            continue;
        };

        let label = {
            let cat = st.catalog.lock().unwrap();
            entry
                .project_id
                .as_deref()
                .and_then(|p| cat.item(p, entry.task_id.as_deref()))
                .map(|i| crate::label(&i))
                .unwrap_or_else(|| "No project".into())
        };
        *st.away.lock().unwrap() = Some(Away {
            start: fmt_time(start),
            end: fmt_time(end),
            minutes: (end - start).num_minutes(),
            entry: entry.local_id,
            label,
        });
        let _ = refresh_tray(&app);
        show_prompt(&app);
    }
}

/// Non-modal prompt in the top-right corner; it does not take focus (no focus stealing).
pub fn show_prompt(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(IDLE_WINDOW) {
        let _ = w.show();
        return;
    }
    let (w, h) = (400.0, 168.0);
    let mut builder =
        WebviewWindowBuilder::new(app, IDLE_WINDOW, WebviewUrl::App("index.html".into()))
            .title("Ira Tracker – Away")
            .inner_size(w, h)
            .resizable(false)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false);
    if let Ok(Some(m)) = app.primary_monitor() {
        let size = m.size().to_logical::<f64>(m.scale_factor());
        let pos = m.position().to_logical::<f64>(m.scale_factor());
        builder = builder.position(pos.x + size.width - w - 16.0, pos.y + 40.0);
    }
    if let Err(e) = builder.build() {
        eprintln!("idle prompt: {e}");
    }
}

#[tauri::command]
pub fn get_away(st: State<AppState>) -> Option<Away> {
    st.away.lock().unwrap().clone()
}

/// `action`: "keep" (count the time), "discard" (remove it), or "assign" (move it to
/// `project_id`/`task_id`). The original timer keeps running in every case.
#[tauri::command]
pub fn resolve_away(
    app: AppHandle,
    action: String,
    project_id: Option<String>,
    task_id: Option<String>,
) -> Result<(), String> {
    let st = app.state::<AppState>();
    let Some(away) = st.away.lock().unwrap().clone() else {
        return Ok(());
    };
    let assign = match action.as_str() {
        "keep" => None,
        "discard" => Some(None),
        "assign" => {
            let p = project_id.ok_or("Pick a project")?;
            let cat = st.catalog.lock().unwrap();
            let Some(project) = cat.projects.iter().find(|x| x.id == p) else {
                return Err("Unknown project; refresh and try again".into());
            };
            if task_id
                .as_ref()
                .is_some_and(|t| !cat.tasks.iter().any(|x| &x.id == t && x.project_id == p))
            {
                return Err("Unknown task for this project".into());
            }
            Some(Some((p.clone(), project.is_billable)))
        }
        other => return Err(format!("Unknown action {other}")),
    };
    if let Some(target) = assign {
        db::cut_running(
            &mut st.db.lock().unwrap(),
            &away.entry,
            &away.start,
            &away.end,
            target
                .as_ref()
                .map(|(p, billable)| (p.as_str(), task_id.as_deref(), *billable)),
        )
        .map_err(|e| e.to_string())?;
    }
    *st.away.lock().unwrap() = None;
    if let Some(w) = app.get_webview_window(IDLE_WINDOW) {
        let _ = w.close();
    }
    changed(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN10: i64 = 600;

    fn t(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).unwrap()
    }

    #[test]
    fn idle_then_return_reports_away_period() {
        let mut d = Detector::default();
        assert_eq!(d.tick(t(0), 30, Some(0), MIN10, t(-3600)), None);
        // still away after 700 s without input: nothing yet, but now in away mode
        assert_eq!(d.tick(t(700), 30, Some(700), MIN10, t(-3600)), None);
        assert!(d.is_away());
        // input 1 s ago: user is back
        assert_eq!(
            d.tick(t(703), 3, Some(1), MIN10, t(-3600)),
            Some((t(0), t(702)))
        );
        assert!(!d.is_away());
    }

    #[test]
    fn short_idle_is_ignored() {
        let mut d = Detector::default();
        assert_eq!(d.tick(t(300), 30, Some(300), MIN10, t(-3600)), None);
        assert_eq!(d.tick(t(303), 3, Some(0), MIN10, t(-3600)), None);
        assert!(!d.is_away());
    }

    #[test]
    fn sleep_is_detected_from_clock_jump() {
        let mut d = Detector::default();
        d.tick(t(0), 30, Some(10), MIN10, t(-3600));
        // machine slept ~1 h; on wake the user touches the keyboard
        let r = d.tick(t(3630), 30, Some(2), MIN10, t(-3600));
        assert_eq!(r, Some((t(-10), t(3628))));
    }

    #[test]
    fn away_never_starts_before_the_timer() {
        // Last input at t(0), timer started at t(50): away is clamped to t(50).
        let mut d = Detector::default();
        d.tick(t(700), 30, Some(700), MIN10, t(50));
        assert_eq!(
            d.tick(t(703), 3, Some(0), MIN10, t(50)),
            Some((t(50), t(703)))
        );

        // Timer started at t(150): only 553 s of away time remain, below the threshold.
        let mut d = Detector::default();
        d.tick(t(700), 30, Some(700), MIN10, t(150));
        assert_eq!(d.tick(t(703), 3, Some(0), MIN10, t(150)), None);
    }

    #[test]
    fn without_os_idle_only_sleep_counts() {
        let mut d = Detector::default();
        assert_eq!(d.tick(t(0), 30, None, MIN10, t(-3600)), None);
        assert_eq!(d.tick(t(30), 30, None, MIN10, t(-3600)), None);
        assert_eq!(
            d.tick(t(2000), 30, None, MIN10, t(-3600)),
            Some((t(30), t(2000)))
        );
    }

    #[test]
    fn skip_resets_so_a_late_start_is_not_mistaken_for_sleep() {
        let mut d = Detector::default();
        d.tick(t(0), 30, Some(0), MIN10, t(-3600));
        d.skip(t(5000));
        assert_eq!(d.tick(t(5030), 30, Some(0), MIN10, t(4990)), None);
        assert!(!d.is_away());
    }
}
