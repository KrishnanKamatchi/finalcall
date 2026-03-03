use std::{collections::HashMap, fs, path::PathBuf, sync::Mutex, time::Duration};

use chrono::{DateTime, Local, LocalResult, NaiveDate, NaiveTime, TimeZone};
use notify_rust::Notification;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, State,
};

const DEFAULT_DAILY_TARGET_MINUTES: i64 = 510;
const DEFAULT_NOTIFY_BEFORE_MINUTES: i64 = 10;
const REMINDER_EVENT: &str = "finalcall://reminder";

#[derive(Default)]
struct SchedulerState {
    handles: HashMap<String, tauri::async_runtime::JoinHandle<()>>,
}

struct AppState {
    db_path: Mutex<Option<PathBuf>>,
    scheduler: Mutex<SchedulerState>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SettingsDto {
    daily_target_minutes: i64,
    notify_before_minutes: i64,
    autostart_enabled: bool,
    start_in_tray: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateSettingsInput {
    daily_target_minutes: Option<i64>,
    notify_before_minutes: Option<i64>,
    autostart_enabled: Option<bool>,
    start_in_tray: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TodayStatusDto {
    date: String,
    has_check_in: bool,
    check_in_at: Option<String>,
    out_time_at: Option<String>,
    remaining_seconds: Option<i64>,
    session_status: Option<String>,
    next_reminder_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WorkSessionDto {
    id: i64,
    work_date: String,
    check_in_at: String,
    out_time_at: String,
    status: String,
    stopped_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ReminderPayload {
    kind: String,
    title: String,
    message: String,
    out_time_at: Option<String>,
}

#[derive(Debug, Clone)]
struct WorkSession {
    id: i64,
    check_in_at: DateTime<Local>,
    out_time_at: DateTime<Local>,
    status: String,
}

fn today_string() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn now_rfc3339() -> String {
    Local::now().to_rfc3339()
}

fn parse_local_rfc3339(value: &str) -> Result<DateTime<Local>, String> {
    let dt = DateTime::parse_from_rfc3339(value)
        .map_err(|e| format!("invalid datetime '{}': {e}", value))?;
    Ok(dt.with_timezone(&Local))
}

fn local_from_date_and_time(date: NaiveDate, time: NaiveTime) -> Result<DateTime<Local>, String> {
    let naive = date.and_time(time);
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => Ok(dt),
        LocalResult::Ambiguous(dt, _) => Ok(dt),
        LocalResult::None => Err("failed to resolve local datetime".to_string()),
    }
}

fn get_db_path(state: &AppState) -> Result<PathBuf, String> {
    let guard = state
        .db_path
        .lock()
        .map_err(|_| "failed to lock app db state".to_string())?;
    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| "app database is not initialized".to_string())
}

fn open_conn(state: &AppState) -> Result<Connection, String> {
    let path = get_db_path(state)?;
    Connection::open(path).map_err(|e| format!("failed to open sqlite database: {e}"))
}

fn init_db(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS settings (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  daily_target_minutes INTEGER NOT NULL DEFAULT 510,
  notify_before_minutes INTEGER NOT NULL DEFAULT 10,
  autostart_enabled INTEGER NOT NULL DEFAULT 1,
  start_in_tray INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS work_sessions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  work_date TEXT NOT NULL UNIQUE,
  check_in_at TEXT NOT NULL,
  check_in_source TEXT NOT NULL,
  out_time_at TEXT NOT NULL,
  status TEXT NOT NULL,
  stopped_at TEXT
);

CREATE TABLE IF NOT EXISTS reminders (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id INTEGER,
  kind TEXT NOT NULL,
  scheduled_for TEXT NOT NULL,
  fired_at TEXT,
  action_taken TEXT,
  FOREIGN KEY(session_id) REFERENCES work_sessions(id)
);

CREATE INDEX IF NOT EXISTS idx_reminders_schedule ON reminders (scheduled_for, fired_at);
"#,
    )
    .map_err(|e| format!("failed to initialize schema: {e}"))?;

    conn.execute(
        "INSERT OR IGNORE INTO settings (id, daily_target_minutes, notify_before_minutes, autostart_enabled, start_in_tray) VALUES (1, ?1, ?2, 1, 1)",
        params![DEFAULT_DAILY_TARGET_MINUTES, DEFAULT_NOTIFY_BEFORE_MINUTES],
    )
    .map_err(|e| format!("failed to seed default settings: {e}"))?;

    Ok(())
}

fn get_settings_inner(conn: &Connection) -> Result<SettingsDto, String> {
    conn.query_row(
        "SELECT daily_target_minutes, notify_before_minutes, autostart_enabled, start_in_tray FROM settings WHERE id = 1",
        [],
        |row| {
            Ok(SettingsDto {
                daily_target_minutes: row.get(0)?,
                notify_before_minutes: row.get(1)?,
                autostart_enabled: row.get::<_, i64>(2)? == 1,
                start_in_tray: row.get::<_, i64>(3)? == 1,
            })
        },
    )
    .map_err(|e| format!("failed to read settings: {e}"))
}

fn update_settings_inner(
    conn: &Connection,
    input: &UpdateSettingsInput,
) -> Result<SettingsDto, String> {
    let mut settings = get_settings_inner(conn)?;

    if let Some(v) = input.daily_target_minutes {
        settings.daily_target_minutes = v.clamp(60, 16 * 60);
    }
    if let Some(v) = input.notify_before_minutes {
        settings.notify_before_minutes = v.clamp(0, 60);
    }
    if let Some(v) = input.autostart_enabled {
        settings.autostart_enabled = v;
    }
    if let Some(v) = input.start_in_tray {
        settings.start_in_tray = v;
    }

    conn.execute(
        "UPDATE settings SET daily_target_minutes = ?1, notify_before_minutes = ?2, autostart_enabled = ?3, start_in_tray = ?4 WHERE id = 1",
        params![
            settings.daily_target_minutes,
            settings.notify_before_minutes,
            settings.autostart_enabled as i64,
            settings.start_in_tray as i64
        ],
    )
    .map_err(|e| format!("failed to update settings: {e}"))?;

    Ok(settings)
}

fn expire_old_active_sessions(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "UPDATE work_sessions SET status = 'expired' WHERE status = 'active' AND work_date < ?1",
        [today_string()],
    )
    .map_err(|e| format!("failed to expire old sessions: {e}"))?;
    Ok(())
}

fn get_session_by_date(conn: &Connection, date: &str) -> Result<Option<WorkSession>, String> {
    conn.query_row(
        "SELECT id, check_in_at, out_time_at, status FROM work_sessions WHERE work_date = ?1",
        [date],
        |row| {
            let check_in_raw: String = row.get(1)?;
            let out_time_raw: String = row.get(2)?;
            Ok(WorkSession {
                id: row.get(0)?,
                check_in_at: parse_local_rfc3339(&check_in_raw).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                    )
                })?,
                out_time_at: parse_local_rfc3339(&out_time_raw).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                    )
                })?,
                status: row.get(3)?,
            })
        },
    )
    .optional()
    .map_err(|e| format!("failed to read work session: {e}"))
}

fn upsert_today_session(
    conn: &Connection,
    check_in_at: DateTime<Local>,
    out_time_at: DateTime<Local>,
    source: &str,
) -> Result<(), String> {
    let work_date = today_string();
    conn.execute(
        "INSERT INTO work_sessions (work_date, check_in_at, check_in_source, out_time_at, status) VALUES (?1, ?2, ?3, ?4, 'active')
         ON CONFLICT(work_date) DO UPDATE SET check_in_at=excluded.check_in_at, check_in_source=excluded.check_in_source, out_time_at=excluded.out_time_at, status='active', stopped_at=NULL",
        params![work_date, check_in_at.to_rfc3339(), source, out_time_at.to_rfc3339()],
    )
    .map_err(|e| format!("failed to write work session: {e}"))?;
    Ok(())
}

fn update_session_status(
    conn: &Connection,
    date: &str,
    status: &str,
    stopped_at: Option<String>,
) -> Result<(), String> {
    conn.execute(
        "UPDATE work_sessions SET status = ?1, stopped_at = ?2 WHERE work_date = ?3",
        params![status, stopped_at, date],
    )
    .map_err(|e| format!("failed to update work session status: {e}"))?;
    Ok(())
}

fn session_to_status(session: Option<WorkSession>) -> TodayStatusDto {
    let date = today_string();
    if let Some(s) = session {
        let now = Local::now();
        let remaining = if s.status == "active" {
            Some((s.out_time_at - now).num_seconds().max(0))
        } else {
            Some(0)
        };
        TodayStatusDto {
            date,
            has_check_in: true,
            check_in_at: Some(s.check_in_at.to_rfc3339()),
            out_time_at: Some(s.out_time_at.to_rfc3339()),
            remaining_seconds: remaining,
            session_status: Some(s.status),
            next_reminder_at: None,
        }
    } else {
        TodayStatusDto {
            date,
            has_check_in: false,
            check_in_at: None,
            out_time_at: None,
            remaining_seconds: None,
            session_status: None,
            next_reminder_at: None,
        }
    }
}

fn insert_reminder_log(
    conn: &Connection,
    session_id: i64,
    kind: &str,
    scheduled_for: DateTime<Local>,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO reminders (session_id, kind, scheduled_for, fired_at, action_taken) VALUES (?1, ?2, ?3, ?4, 'none')",
        params![session_id, kind, scheduled_for.to_rfc3339(), now_rfc3339()],
    )
    .map_err(|e| format!("failed to insert reminder log: {e}"))?;
    Ok(())
}

fn should_prompt_for_checkin(conn: &Connection) -> Result<bool, String> {
    let date = today_string();
    let has_session = get_session_by_date(conn, &date)?.is_some();
    Ok(!has_session)
}

fn cancel_scheduler_key(state: &AppState, key: &str) {
    if let Ok(mut scheduler) = state.scheduler.lock() {
        if let Some(handle) = scheduler.handles.remove(key) {
            handle.abort();
        }
    }
}

fn schedule_handle(state: &AppState, key: String, handle: tauri::async_runtime::JoinHandle<()>) {
    if let Ok(mut scheduler) = state.scheduler.lock() {
        if let Some(old) = scheduler.handles.insert(key, handle) {
            old.abort();
        }
    }
}

fn is_today_active(state: &AppState) -> Result<Option<WorkSession>, String> {
    let conn = open_conn(state)?;
    expire_old_active_sessions(&conn)?;
    let date = today_string();
    let session = get_session_by_date(&conn, &date)?;
    Ok(session.filter(|s| s.status == "active"))
}

fn emit_reminder(app: &AppHandle, payload: ReminderPayload) {
    let _ = app.emit(REMINDER_EVENT, payload);
}

fn show_os_notification(title: &str, body: &str) {
    let _ = Notification::new().summary(title).body(body).show();
}

fn schedule_repeat_loop(app: AppHandle, first_fire_at: DateTime<Local>) {
    let task_app = app.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let mut next_fire = first_fire_at;

        loop {
            let now = Local::now();
            let wait = (next_fire - now)
                .to_std()
                .unwrap_or_else(|_| Duration::from_secs(0));
            tokio::time::sleep(wait).await;

            let state = task_app.state::<AppState>();
            let active_session = match is_today_active(&state) {
                Ok(session) => session,
                Err(_) => break,
            };

            if let Some(session) = active_session {
                let title = "FinalCall: Office Time Complete";
                let body = "Your planned office time is done. Stop for today or snooze 10 minutes.";
                show_os_notification(title, body);

                if let Ok(conn) = open_conn(&state) {
                    let _ = insert_reminder_log(&conn, session.id, "out_notify", next_fire);
                }

                emit_reminder(
                    &task_app,
                    ReminderPayload {
                        kind: "outTime".to_string(),
                        title: title.to_string(),
                        message: body.to_string(),
                        out_time_at: Some(session.out_time_at.to_rfc3339()),
                    },
                );

                next_fire = Local::now() + chrono::Duration::minutes(10);
            } else {
                break;
            }
        }
    });

    let state = app.state::<AppState>();
    schedule_handle(&state, "repeat-loop".to_string(), handle);
}

fn schedule_for_today(app: &AppHandle, session: &WorkSession, settings: &SettingsDto) {
    let state = app.state::<AppState>();

    cancel_scheduler_key(&state, "pre-notify");
    cancel_scheduler_key(&state, "repeat-loop");

    if session.status != "active" {
        return;
    }

    let now = Local::now();
    if settings.notify_before_minutes > 0 {
        let pre_at =
            session.out_time_at - chrono::Duration::minutes(settings.notify_before_minutes);
        if pre_at > now {
            let app_handle = app.clone();
            let session_id = session.id;
            let handle = tauri::async_runtime::spawn(async move {
                let delay = (pre_at - Local::now())
                    .to_std()
                    .unwrap_or_else(|_| Duration::from_secs(0));
                tokio::time::sleep(delay).await;

                let title = "FinalCall: Wrapping Up Soon";
                let msg = "You are close to your planned out time.";
                show_os_notification(title, msg);

                let state = app_handle.state::<AppState>();
                if let Ok(conn) = open_conn(&state) {
                    let _ = insert_reminder_log(&conn, session_id, "pre_notify", pre_at);
                }

                emit_reminder(
                    &app_handle,
                    ReminderPayload {
                        kind: "preNotify".to_string(),
                        title: title.to_string(),
                        message: msg.to_string(),
                        out_time_at: Some((pre_at + chrono::Duration::minutes(0)).to_rfc3339()),
                    },
                );
            });
            schedule_handle(&state, "pre-notify".to_string(), handle);
        }
    }

    let first_out_fire = if session.out_time_at > now {
        session.out_time_at
    } else {
        now
    };
    schedule_repeat_loop(app.clone(), first_out_fire);
}

fn setup_linux_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if !cfg!(target_os = "linux") {
        return Ok(());
    }

    let home = std::env::var("HOME").map_err(|e| format!("failed to read HOME: {e}"))?;
    let autostart_dir = PathBuf::from(home).join(".config/autostart");
    fs::create_dir_all(&autostart_dir)
        .map_err(|e| format!("failed to create autostart directory: {e}"))?;

    let desktop_file = autostart_dir.join("com.krish.finalcall.desktop");

    if enabled {
        let exe =
            std::env::current_exe().map_err(|e| format!("failed to read current exe path: {e}"))?;
        let content = format!(
            "[Desktop Entry]\nType=Application\nName=FinalCall\nExec={}\nX-GNOME-Autostart-enabled=true\nTerminal=false\n",
            exe.display()
        );
        fs::write(&desktop_file, content)
            .map_err(|e| format!("failed to write autostart desktop entry: {e}"))?;
    } else if desktop_file.exists() {
        fs::remove_file(&desktop_file)
            .map_err(|e| format!("failed to remove autostart desktop entry: {e}"))?;
    }

    let _ = app;
    Ok(())
}

fn build_today_status(state: &AppState) -> Result<TodayStatusDto, String> {
    let conn = open_conn(state)?;
    expire_old_active_sessions(&conn)?;
    let session = get_session_by_date(&conn, &today_string())?;
    Ok(session_to_status(session))
}

fn execute_checkin(
    app: &AppHandle,
    state: &AppState,
    check_in_at: DateTime<Local>,
    source: &str,
    pre_notify_minutes: Option<i64>,
) -> Result<TodayStatusDto, String> {
    let conn = open_conn(state)?;
    expire_old_active_sessions(&conn)?;

    let mut settings = get_settings_inner(&conn)?;
    if let Some(minutes) = pre_notify_minutes {
        settings.notify_before_minutes = minutes.clamp(0, 60);
        conn.execute(
            "UPDATE settings SET notify_before_minutes = ?1 WHERE id = 1",
            [settings.notify_before_minutes],
        )
        .map_err(|e| format!("failed to update pre-notify minutes: {e}"))?;
    }

    let out_time = check_in_at + chrono::Duration::minutes(settings.daily_target_minutes);
    upsert_today_session(&conn, check_in_at, out_time, source)?;

    let today_session = get_session_by_date(&conn, &today_string())?
        .ok_or_else(|| "failed to read session after check-in".to_string())?;

    schedule_for_today(app, &today_session, &settings);

    Ok(session_to_status(Some(today_session)))
}

#[tauri::command]
fn get_today_status(state: State<'_, AppState>) -> Result<TodayStatusDto, String> {
    build_today_status(&state)
}

#[tauri::command]
fn check_in_now(
    app: AppHandle,
    state: State<'_, AppState>,
    pre_notify_minutes: Option<i64>,
) -> Result<TodayStatusDto, String> {
    execute_checkin(&app, &state, Local::now(), "now", pre_notify_minutes)
}

#[tauri::command]
fn check_in_manual(
    app: AppHandle,
    state: State<'_, AppState>,
    local_time_hhmm: String,
    pre_notify_minutes: Option<i64>,
) -> Result<TodayStatusDto, String> {
    let time = NaiveTime::parse_from_str(&local_time_hhmm, "%H:%M")
        .map_err(|_| "time must be in HH:MM 24-hour format".to_string())?;
    let date = Local::now().date_naive();
    let check_in = local_from_date_and_time(date, time)?;

    execute_checkin(&app, &state, check_in, "manual", pre_notify_minutes)
}

fn stop_today_inner(state: &AppState) -> Result<TodayStatusDto, String> {
    let conn = open_conn(state)?;
    expire_old_active_sessions(&conn)?;

    let date = today_string();
    let session = get_session_by_date(&conn, &date)?;
    if session.is_some() {
        update_session_status(&conn, &date, "stopped", Some(now_rfc3339()))?;
    }

    cancel_scheduler_key(state, "pre-notify");
    cancel_scheduler_key(state, "repeat-loop");

    let refreshed = get_session_by_date(&conn, &date)?;
    Ok(session_to_status(refreshed))
}

#[tauri::command]
fn stop_today(state: State<'_, AppState>) -> Result<TodayStatusDto, String> {
    stop_today_inner(&state)
}

#[tauri::command]
fn snooze_today(
    app: AppHandle,
    state: State<'_, AppState>,
    minutes: Option<i64>,
) -> Result<TodayStatusDto, String> {
    let snooze_mins = minutes.unwrap_or(10).clamp(1, 60);
    let conn = open_conn(&state)?;
    let date = today_string();
    let session = get_session_by_date(&conn, &date)?;
    let Some(session) = session else {
        return Err("no check-in session for today".to_string());
    };
    if session.status != "active" {
        return Err("today session is not active".to_string());
    }

    cancel_scheduler_key(&state, "repeat-loop");
    let fire_at = Local::now() + chrono::Duration::minutes(snooze_mins);

    conn.execute(
        "INSERT INTO reminders (session_id, kind, scheduled_for, action_taken) VALUES (?1, 'snooze_notify', ?2, 'snooze')",
        params![session.id, fire_at.to_rfc3339()],
    )
    .map_err(|e| format!("failed to save snooze log: {e}"))?;

    schedule_repeat_loop(app, fire_at);
    Ok(session_to_status(Some(session)))
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<SettingsDto, String> {
    let conn = open_conn(&state)?;
    get_settings_inner(&conn)
}

#[tauri::command]
fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    input: UpdateSettingsInput,
) -> Result<SettingsDto, String> {
    let conn = open_conn(&state)?;
    let settings = update_settings_inner(&conn, &input)?;
    setup_linux_autostart(&app, settings.autostart_enabled)?;

    let session = get_session_by_date(&conn, &today_string())?;
    if let Some(s) = session {
        schedule_for_today(&app, &s, &settings);
    }

    Ok(settings)
}

#[tauri::command]
fn ensure_autostart(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<bool, String> {
    let conn = open_conn(&state)?;
    conn.execute(
        "UPDATE settings SET autostart_enabled = ?1 WHERE id = 1",
        [enabled as i64],
    )
    .map_err(|e| format!("failed to persist autostart preference: {e}"))?;

    setup_linux_autostart(&app, enabled)?;
    Ok(enabled)
}

#[tauri::command]
fn get_history(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<WorkSessionDto>, String> {
    let conn = open_conn(&state)?;
    let max_rows = limit.unwrap_or(14).clamp(1, 120);

    let mut stmt = conn
        .prepare(
            "SELECT id, work_date, check_in_at, out_time_at, status, stopped_at
             FROM work_sessions
             ORDER BY work_date DESC
             LIMIT ?1",
        )
        .map_err(|e| format!("failed to prepare history query: {e}"))?;

    let rows = stmt
        .query_map([max_rows], |row| {
            Ok(WorkSessionDto {
                id: row.get(0)?,
                work_date: row.get(1)?,
                check_in_at: row.get(2)?,
                out_time_at: row.get(3)?,
                status: row.get(4)?,
                stopped_at: row.get(5)?,
            })
        })
        .map_err(|e| format!("failed to read history rows: {e}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to build history data: {e}"))
}

#[tauri::command]
fn open_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    } else {
        return Err("main window not found".to_string());
    }

    if let Some(mini) = app.get_webview_window("mini") {
        let _ = mini.hide();
    }

    Ok(())
}

fn setup_db_and_scheduler(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data directory: {e}"))?;
    fs::create_dir_all(&app_data_dir)
        .map_err(|e| format!("failed to create app data directory: {e}"))?;

    let db_path = app_data_dir.join("finalcall.sqlite3");
    {
        let mut db_guard = state
            .db_path
            .lock()
            .map_err(|_| "failed to lock app db state".to_string())?;
        *db_guard = Some(db_path);
    }

    let conn = open_conn(&state)?;
    init_db(&conn)?;
    expire_old_active_sessions(&conn)?;

    let settings = get_settings_inner(&conn)?;
    setup_linux_autostart(app, settings.autostart_enabled)?;

    if let Some(session) = get_session_by_date(&conn, &today_string())? {
        schedule_for_today(app, &session, &settings);
    }

    Ok(())
}

fn maybe_show_window_for_checkin(app: &AppHandle, tray_enabled: bool) {
    let state = app.state::<AppState>();
    let Ok(conn) = open_conn(&state) else {
        return;
    };

    let settings = match get_settings_inner(&conn) {
        Ok(v) => v,
        Err(_) => return,
    };

    let should_prompt = should_prompt_for_checkin(&conn).unwrap_or(false);
    let app_handle = app.clone();

    // Delay initial window visibility changes until GTK window is fully initialized.
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;

        let app_for_main = app_handle.clone();
        let _ = app_handle.run_on_main_thread(move || {
            let main = app_for_main.get_webview_window("main");
            let mini = app_for_main.get_webview_window("mini");

            if settings.start_in_tray && !should_prompt && tray_enabled {
                if let Some(main_window) = main {
                    let _ = main_window.hide();
                }
                if let Some(mini_window) = mini {
                    let _ = mini_window.hide();
                }
                return;
            }

            if let Some(main_window) = main {
                let _ = main_window.hide();
            }

            if let Some(mini_window) = mini {
                let _ = mini_window.show();
                if should_prompt {
                    let _ = mini_window.set_focus();
                }
            }
        });
    });
}

fn setup_tray(app: &AppHandle) -> Result<(), String> {
    let open_item = MenuItem::with_id(app, "open", "Open FinalCall", true, None::<&str>)
        .map_err(|e| format!("failed to create tray menu item: {e}"))?;
    let checkin_item = MenuItem::with_id(app, "checkin", "Check In Now", true, None::<&str>)
        .map_err(|e| format!("failed to create tray menu item: {e}"))?;
    let stop_item = MenuItem::with_id(app, "stop", "Stop Today", true, None::<&str>)
        .map_err(|e| format!("failed to create tray menu item: {e}"))?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)
        .map_err(|e| format!("failed to create tray menu item: {e}"))?;

    let menu = Menu::with_items(app, &[&open_item, &checkin_item, &stop_item, &quit_item])
        .map_err(|e| format!("failed to build tray menu: {e}"))?;

    TrayIconBuilder::new()
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
                if let Some(window) = app.get_webview_window("mini") {
                    let _ = window.hide();
                }
            }
            "checkin" => {
                let app_handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app_handle.state::<AppState>();
                    let _ = execute_checkin(&app_handle, &state, Local::now(), "now", None);
                });
            }
            "stop" => {
                let state = app.state::<AppState>();
                let _ = stop_today_inner(&state);
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)
        .map_err(|e| format!("failed to build tray icon: {e}"))?;

    Ok(())
}

fn should_setup_tray() -> bool {
    // On Linux debug runs, tray initialization can trigger noisy GTK critical logs.
    // Keep tray enabled in release builds; allow explicit opt-in in debug via env var.
    if cfg!(all(target_os = "linux", debug_assertions)) {
        return std::env::var("FINALCALL_ENABLE_TRAY_DEV")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
    }
    true
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            db_path: Mutex::new(None),
            scheduler: Mutex::new(SchedulerState::default()),
        })
        .setup(|app| {
            setup_db_and_scheduler(app.handle())?;
            let tray_enabled = should_setup_tray();
            if tray_enabled {
                setup_tray(app.handle())?;
            }
            maybe_show_window_for_checkin(app.handle(), tray_enabled);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_today_status,
            check_in_now,
            check_in_manual,
            stop_today,
            snooze_today,
            get_settings,
            update_settings,
            get_history,
            ensure_autostart,
            open_main_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
