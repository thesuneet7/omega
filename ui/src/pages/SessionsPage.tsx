import { useCallback, useEffect, useState } from "react";
import { BucketSummaryWorkspace } from "../components/BucketSummaryWorkspace";
import { RevisionHistory } from "../components/RevisionHistory";
import { SummaryEditor } from "../components/SummaryEditor";
import { ApiUsageMeter } from "../components/ApiUsageMeter";
import { CaptureLiveStatusBanner } from "../components/CaptureLiveStatusBanner";
import { DataManifestPanel } from "../components/DataManifestPanel";
import { ExcludedAppsPanel } from "../components/ExcludedAppsPanel";
import {
  endSession,
  getSessionSummary,
  listSessions,
  saveSummaryRevision,
  setCapturePaused as persistCapturePause,
  startSession,
  type Phase1LiveStatus,
  type SessionListItem,
  type SessionSummaryState,
  type SummaryRevision,
} from "../lib/api";
import { sessionListPrimaryLine, sessionListSecondaryLine } from "../lib/sessionDisplay";

/** After refresh, keep the same session if it still exists; otherwise fall back to the first row. */
function resolveSelectedSession(
  prev: SessionListItem | null,
  sessions: SessionListItem[]
): SessionListItem | null {
  if (prev) {
    const stillThere = sessions.find((s) => s.session_key === prev.session_key);
    if (stillThere) return stillThere;
  }
  return sessions[0] ?? null;
}

type SessionState = "capturing" | "processing" | "idle";

function statusDotClass(state: SessionState, privacyPaused: boolean): string {
  if (state === "capturing" && privacyPaused) return "status-dot status-dot--paused";
  if (state === "capturing") return "status-dot status-dot--capturing";
  if (state === "processing") return "status-dot status-dot--processing";
  return "status-dot status-dot--idle";
}

export function SessionsPage() {
  const [sessions, setSessions] = useState<SessionListItem[]>([]);
  const [selected, setSelected] = useState<SessionListItem | null>(null);
  const [summary, setSummary] = useState<SessionSummaryState | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sessionState, setSessionState] = useState<SessionState>("idle");
  const [privacyPaused, setPrivacyPaused] = useState(false);
  const [usageTick, setUsageTick] = useState(0);

  const refreshSessions = useCallback(async () => {
    setLoading(true);
    try {
      const data = await listSessions();
      setSessions(data);
      setSelected((prev) => resolveSelectedSession(prev, data));
      setError(null);
      setUsageTick((t) => t + 1);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshSessions();
  }, [refreshSessions]);

  useEffect(() => {
    if (!selected) {
      setSummary(null);
      return;
    }
    void getSessionSummary(selected.session_key).then(setSummary).catch((e: Error) => {
      setError(e.message);
    });
  }, [selected]);

  const handleSave = useCallback(
    async (title: string, body: string, autosave: boolean) => {
      if (!selected) return;
      setSaving(true);
      try {
        const updated = await saveSummaryRevision(
          selected.session_key,
          title,
          body,
          autosave ? "autosave" : "manual-save"
        );
        setSummary(updated);
      } finally {
        setSaving(false);
      }
    },
    [selected]
  );

  const handleRestore = useCallback(
    async (rev: SummaryRevision) => {
      if (!selected) return;
      const updated = await saveSummaryRevision(
        selected.session_key,
        rev.title,
        rev.body,
        "restore-revision"
      );
      setSummary(updated);
    },
    [selected]
  );

  const handleStartSession = useCallback(async () => {
    setError(null);
    try {
      await startSession();
      setPrivacyPaused(false);
      setSessionState("capturing");
    } catch (e) {
      setError((e as Error).message);
    }
  }, []);

  const handleLiveStatus = useCallback((live: Phase1LiveStatus | null) => {
    setPrivacyPaused(live?.capturePaused === true);
  }, []);

  const applyCapturePause = useCallback(async (paused: boolean) => {
    setError(null);
    try {
      const s = await persistCapturePause(paused);
      setPrivacyPaused(!!s.capturePaused);
    } catch (e) {
      setError((e as Error).message);
    }
  }, []);

  const handleEndSession = useCallback(async () => {
    setSessionState("processing");
    setError(null);
    try {
      await endSession();
      const data = await listSessions();
      setSessions(data);
      setSelected((prev) => resolveSelectedSession(prev, data));
      setPrivacyPaused(false);
      setSessionState("idle");
      setUsageTick((t) => t + 1);
    } catch (e) {
      setError((e as Error).message);
      setSessionState("idle");
    }
  }, []);

  const statusMessage =
    sessionState === "capturing"
      ? privacyPaused
        ? "Privacy pause — capture is off until you resume (tray or ⌘⇧9 in the desktop app)."
        : "Capture is on — your activity is being recorded."
      : sessionState === "processing"
        ? "Wrapping up: ingesting, stitching, and summarizing…"
        : "Idle — start a session when you’re ready to capture again.";

  return (
    <main className="layout">
      <aside className="panel panel--sidebar">
        <div className="panel__header sidebar__header">
          <div className="brand">
            <span className="brand__name">Omega</span>
            <h2 className="brand__title">Your sessions</h2>
          </div>
          <button type="button" className="btn-ghost btn-small sidebar__refresh" onClick={() => void refreshSessions()}>
            Refresh
          </button>
        </div>
        {loading ? <p className="loading-hint">Loading sessions…</p> : null}
        {error ? <p className="error">{error}</p> : null}

        <div className="sidebar__sessions">
          <h3 className="sidebar__section-title">Sessions</h3>
          <div className="list list--sessions">
            {sessions.length === 0 && !loading ? (
              <p className="empty-hint">No sessions yet. End a capture run to see it here.</p>
            ) : null}
            {sessions.map((s) => {
              const primary = sessionListPrimaryLine(s, {
                selectedSessionKey: selected?.session_key ?? null,
                currentSummaryTitle: summary?.title ?? null,
              });
              return (
                <button
                  type="button"
                  key={s.session_key}
                  className={`list-item buttonish ${selected?.session_key === s.session_key ? "active" : ""}`}
                  onClick={() => setSelected(s)}
                >
                  <div>
                    <span className="session-list__primary">{primary}</span>
                    <div className="muted session-list__meta">{sessionListSecondaryLine(s, primary)}</div>
                  </div>
                </button>
              );
            })}
          </div>
        </div>

        <div className="sidebar__footer">
          <ApiUsageMeter
            sessionKey={selected?.session_key ?? null}
            sessionLabel={
              selected
                ? sessionListPrimaryLine(selected, {
                    selectedSessionKey: selected.session_key,
                    currentSummaryTitle: summary?.title ?? null,
                  })
                : null
            }
            refreshToken={usageTick}
          />
          <ExcludedAppsPanel />
          <DataManifestPanel
            selectedSessionKey={selected?.session_key ?? null}
            onDataChanged={() => void refreshSessions()}
          />
        </div>
      </aside>

      <section className="content">
        <section className="panel panel--recording">
          <div className="recording-panel__row">
            <div className="recording-panel__info">
              <h2 className="session-headline">Recording</h2>
              <div className="session-status-row">
                <span
                  className={statusDotClass(sessionState, privacyPaused)}
                  aria-hidden
                />
                <p className="status-text">{statusMessage}</p>
              </div>
              <CaptureLiveStatusBanner
                active={sessionState === "capturing"}
                onLiveStatus={handleLiveStatus}
              />
            </div>
            <div className="recording-actions" role="group" aria-label="Session controls">
              <button type="button" onClick={() => void handleStartSession()} disabled={sessionState !== "idle"}>
                Start session
              </button>
              {sessionState === "capturing" && !privacyPaused ? (
                <button type="button" onClick={() => void applyCapturePause(true)}>
                  Pause capture
                </button>
              ) : null}
              {sessionState === "capturing" && privacyPaused ? (
                <button type="button" className="primary-button" onClick={() => void applyCapturePause(false)}>
                  Resume capture
                </button>
              ) : null}
              <button
                type="button"
                className={
                  sessionState === "capturing" && !privacyPaused ? "primary-button" : undefined
                }
                onClick={() => void handleEndSession()}
                disabled={sessionState !== "capturing"}
              >
                {sessionState === "processing" ? "Processing…" : "End session"}
              </button>
            </div>
          </div>
        </section>

        {!summary ? (
          <section className="panel">
            <p className="empty-hint">Choose a session on the left to open its summary.</p>
          </section>
        ) : selected && (summary.buckets ?? []).length > 0 ? (
          <BucketSummaryWorkspace
            sessionKey={selected.session_key}
            sessionTitle={summary.title}
            buckets={summary.buckets ?? []}
            revisions={summary.revisions}
            saving={saving}
            onSave={handleSave}
            onRestore={handleRestore}
          />
        ) : (
          <>
            <SummaryEditor
              initialTitle={summary.title}
              initialBody={summary.body}
              onSave={handleSave}
              saving={saving}
            />
            <RevisionHistory revisions={summary.revisions} onRestore={handleRestore} />
          </>
        )}
      </section>
    </main>
  );
}
