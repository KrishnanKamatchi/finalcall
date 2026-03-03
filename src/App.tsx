import { FormEvent, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

type SettingsDto = {
  dailyTargetMinutes: number;
  notifyBeforeMinutes: number;
  autostartEnabled: boolean;
  startInTray: boolean;
};

type TodayStatusDto = {
  date: string;
  hasCheckIn: boolean;
  checkInAt?: string;
  outTimeAt?: string;
  remainingSeconds?: number;
  sessionStatus?: string;
};

type WorkSessionDto = {
  id: number;
  workDate: string;
  checkInAt: string;
  outTimeAt: string;
  status: string;
  stoppedAt?: string;
};

type ReminderPayload = {
  kind: string;
  title: string;
  message: string;
  outTimeAt?: string;
};

const minuteOptions = [5, 10, 20];
const isMiniMode = new URLSearchParams(window.location.search).get("mini") === "1";

function formatDateTime(value?: string) {
  if (!value) return "-";
  return new Date(value).toLocaleString();
}

function formatTimeOnly(value?: string) {
  if (!value) return "--:--";
  return new Date(value).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function formatDuration(seconds?: number) {
  if (seconds === undefined) return "--:--:--";
  const safe = Math.max(0, seconds);
  const h = Math.floor(safe / 3600)
    .toString()
    .padStart(2, "0");
  const m = Math.floor((safe % 3600) / 60)
    .toString()
    .padStart(2, "0");
  const s = Math.floor(safe % 60)
    .toString()
    .padStart(2, "0");
  return `${h}:${m}:${s}`;
}

function App() {
  const [status, setStatus] = useState<TodayStatusDto | null>(null);
  const [settings, setSettings] = useState<SettingsDto | null>(null);
  const [history, setHistory] = useState<WorkSessionDto[]>([]);

  const [manualTime, setManualTime] = useState("09:00");
  const [checkinNotifyBefore, setCheckinNotifyBefore] = useState(10);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reminder, setReminder] = useState<ReminderPayload | null>(null);

  async function refreshAll() {
    try {
      setError(null);
      const [today, appSettings, sessionHistory] = await Promise.all([
        invoke<TodayStatusDto>("get_today_status"),
        invoke<SettingsDto>("get_settings"),
        invoke<WorkSessionDto[]>("get_history", { limit: 10 }),
      ]);
      setStatus(today);
      setSettings(appSettings);
      setHistory(sessionHistory);
      setCheckinNotifyBefore(appSettings.notifyBeforeMinutes);
    } catch (err) {
      setError(String(err));
    }
  }

  useEffect(() => {
    refreshAll();

    const timer = setInterval(async () => {
      try {
        const today = await invoke<TodayStatusDto>("get_today_status");
        setStatus(today);
      } catch {
        // ignore transient polling failures
      }
    }, 1000);

    let unlisten: (() => void) | null = null;
    listen<ReminderPayload>("finalcall://reminder", (event) => {
      setReminder(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      clearInterval(timer);
      if (unlisten) unlisten();
    };
  }, []);

  const showCheckinCard = useMemo(() => {
    return !status?.hasCheckIn || status?.sessionStatus === "expired";
  }, [status]);

  const progress = useMemo(() => {
    if (!status?.checkInAt || settings == null) return 0;
    const elapsedMs = Date.now() - new Date(status.checkInAt).getTime();
    const totalMs = settings.dailyTargetMinutes * 60 * 1000;
    return Math.min(100, Math.max(0, Math.floor((elapsedMs / totalMs) * 100)));
  }, [settings, status?.checkInAt]);

  async function handleCheckInNow() {
    try {
      setSaving(true);
      setError(null);
      const updated = await invoke<TodayStatusDto>("check_in_now", {
        pre_notify_minutes: checkinNotifyBefore,
      });
      setStatus(updated);
      await refreshAll();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleCheckInManual(e: FormEvent) {
    e.preventDefault();
    try {
      setSaving(true);
      setError(null);
      const updated = await invoke<TodayStatusDto>("check_in_manual", {
        local_time_hhmm: manualTime,
        pre_notify_minutes: checkinNotifyBefore,
      });
      setStatus(updated);
      await refreshAll();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleStopToday() {
    try {
      setSaving(true);
      setError(null);
      const updated = await invoke<TodayStatusDto>("stop_today");
      setStatus(updated);
      setReminder(null);
      await refreshAll();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleSnooze() {
    try {
      setSaving(true);
      setError(null);
      const updated = await invoke<TodayStatusDto>("snooze_today", { minutes: 10 });
      setStatus(updated);
      setReminder(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  async function saveSettings(e: FormEvent) {
    e.preventDefault();
    if (!settings) return;

    try {
      setSaving(true);
      setError(null);
      const updated = await invoke<SettingsDto>("update_settings", {
        input: {
          dailyTargetMinutes: settings.dailyTargetMinutes,
          notifyBeforeMinutes: settings.notifyBeforeMinutes,
          autostartEnabled: settings.autostartEnabled,
          startInTray: settings.startInTray,
        },
      });
      setSettings(updated);
      setCheckinNotifyBefore(updated.notifyBeforeMinutes);
      await refreshAll();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleOpenMainWindow() {
    try {
      await invoke("open_main_window");
    } catch (err) {
      setError(String(err));
    }
  }

  if (isMiniMode) {
    return (
      <main className="app-shell mini-shell">
        <div className="bg-orb orb-a" />
        <section className="glass-card">
          <div className="card-head">
            <h2>FinalCall Mini</h2>
            <span className="chip">{status?.sessionStatus ?? "not-started"}</span>
          </div>
          <div className="stats-grid">
            <article>
              <p>Check-in</p>
              <strong>{formatTimeOnly(status?.checkInAt)}</strong>
            </article>
            <article>
              <p>Out-time</p>
              <strong>{formatTimeOnly(status?.outTimeAt)}</strong>
            </article>
            <article>
              <p>Remaining</p>
              <strong>{formatDuration(status?.remainingSeconds)}</strong>
            </article>
            <article>
              <p>Date</p>
              <strong>{status?.date ?? "-"}</strong>
            </article>
          </div>
          <div className="btn-row">
            <button onClick={handleCheckInNow} disabled={saving} className="btn-primary">
              Check In
            </button>
            <button onClick={handleStopToday} disabled={saving || !status?.hasCheckIn} className="btn-ghost">
              Stop
            </button>
            <button onClick={handleSnooze} disabled={saving || !status?.hasCheckIn} className="btn-ghost">
              Snooze
            </button>
          </div>
          <div className="btn-row">
            <button onClick={handleOpenMainWindow} className="btn-secondary">
              Open Full App
            </button>
          </div>
          {error && <p className="error-banner">{error}</p>}
        </section>
      </main>
    );
  }

  return (
    <main className="app-shell">
      <div className="bg-orb orb-a" />
      <div className="bg-orb orb-b" />

      <header className="hero-panel">
        <div>
          <p className="kicker">Work-life Guardian</p>
          <h1>FinalCall</h1>
          <p className="hero-sub">Clock in once. Leave on time, every time.</p>
        </div>
        <div className="hero-clock">{formatDuration(status?.remainingSeconds)}</div>
      </header>

      {error && <p className="error-banner">{error}</p>}

      {reminder && (
        <section className="glass-card reminder-card">
          <h2>{reminder.title}</h2>
          <p>{reminder.message}</p>
          <div className="btn-row">
            <button onClick={handleStopToday} disabled={saving} className="btn-primary">
              Stop Today
            </button>
            <button onClick={handleSnooze} disabled={saving} className="btn-ghost">
              Snooze 10m
            </button>
          </div>
        </section>
      )}

      <section className="glass-card">
        <div className="card-head">
          <h2>Today Console</h2>
          <span className="chip">{status?.sessionStatus ?? "not-started"}</span>
        </div>
        <div className="progress-track">
          <div className="progress-fill" style={{ width: `${progress}%` }} />
        </div>
        <div className="stats-grid">
          <article>
            <p>Check-in</p>
            <strong>{formatTimeOnly(status?.checkInAt)}</strong>
          </article>
          <article>
            <p>Out-time</p>
            <strong>{formatTimeOnly(status?.outTimeAt)}</strong>
          </article>
          <article>
            <p>Date</p>
            <strong>{status?.date ?? "-"}</strong>
          </article>
          <article>
            <p>Remaining</p>
            <strong>{formatDuration(status?.remainingSeconds)}</strong>
          </article>
        </div>
        <div className="btn-row">
          <button onClick={handleStopToday} disabled={saving || !status?.hasCheckIn} className="btn-primary">
            Stop Day
          </button>
          <button onClick={handleSnooze} disabled={saving || !status?.hasCheckIn} className="btn-ghost">
            Snooze 10m
          </button>
        </div>
      </section>

      {showCheckinCard && (
        <section className="glass-card">
          <div className="card-head">
            <h2>Check-in Control</h2>
            <span className="chip chip-warn">Required</span>
          </div>
          <p className="muted">No check-in found for {status?.date ?? "today"}. Set your in-time.</p>
          <div className="btn-row">
            <button onClick={handleCheckInNow} disabled={saving} className="btn-primary">
              Check In Now
            </button>
          </div>
          <form onSubmit={handleCheckInManual} className="form-grid">
            <label>
              Manual time
              <input
                type="time"
                value={manualTime}
                onChange={(e) => setManualTime(e.target.value)}
                required
              />
            </label>
            <label>
              Notify before out-time
              <select
                value={checkinNotifyBefore}
                onChange={(e) => setCheckinNotifyBefore(Number(e.target.value))}
              >
                {minuteOptions.map((m) => (
                  <option key={m} value={m}>
                    {m} min
                  </option>
                ))}
              </select>
            </label>
            <button type="submit" disabled={saving} className="btn-ghost">
              Save Manual Check-in
            </button>
          </form>
        </section>
      )}

      <section className="glass-card">
        <h2>Preferences</h2>
        {settings && (
          <form onSubmit={saveSettings} className="form-grid">
            <label>
              Daily target (minutes)
              <input
                type="number"
                min={60}
                max={960}
                value={settings.dailyTargetMinutes}
                onChange={(e) => setSettings({ ...settings, dailyTargetMinutes: Number(e.target.value) })}
              />
            </label>

            <label>
              Default pre-notify
              <select
                value={settings.notifyBeforeMinutes}
                onChange={(e) => setSettings({ ...settings, notifyBeforeMinutes: Number(e.target.value) })}
              >
                {minuteOptions.map((m) => (
                  <option key={m} value={m}>
                    {m} min
                  </option>
                ))}
              </select>
            </label>

            <label className="toggle-row">
              <input
                type="checkbox"
                checked={settings.autostartEnabled}
                onChange={(e) => setSettings({ ...settings, autostartEnabled: e.target.checked })}
              />
              Auto-start with login
            </label>

            <label className="toggle-row">
              <input
                type="checkbox"
                checked={settings.startInTray}
                onChange={(e) => setSettings({ ...settings, startInTray: e.target.checked })}
              />
              Start in tray
            </label>

            <button type="submit" disabled={saving} className="btn-primary">
              Save Preferences
            </button>
          </form>
        )}
      </section>

      <section className="glass-card">
        <h2>Session Log</h2>
        {history.length === 0 ? (
          <p className="muted">No sessions yet.</p>
        ) : (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Date</th>
                  <th>Check-in</th>
                  <th>Out-time</th>
                  <th>Status</th>
                </tr>
              </thead>
              <tbody>
                {history.map((row) => (
                  <tr key={row.id}>
                    <td>{row.workDate}</td>
                    <td>{formatDateTime(row.checkInAt)}</td>
                    <td>{formatDateTime(row.outTimeAt)}</td>
                    <td>{row.status}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </main>
  );
}

export default App;
