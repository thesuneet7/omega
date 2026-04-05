import { useCallback, useEffect, useState } from "react";
import { BucketSummaryWorkspace } from "../components/BucketSummaryWorkspace";
import { RevisionHistory } from "../components/RevisionHistory";
import { SummaryEditor } from "../components/SummaryEditor";
import {
  endSession,
  getSessionSummary,
  listSessions,
  saveSummaryRevision,
  startSession,
  type SessionListItem,
  type SessionSummaryState,
  type SummaryRevision,
} from "../lib/api";

type SessionState = "capturing" | "processing" | "idle";

function statusDotClass(state: SessionState): string {
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

  const refreshSessions = useCallback(async () => {
    setLoading(true);
    try {
      const data = await listSessions();
      setSessions(data);
      setSelected((prev) => prev ?? data[0] ?? null);
      setError(null);
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
      setSessionState("capturing");
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
      setSelected(data[0] ?? null);
      setSessionState("idle");
    } catch (e) {
      setError((e as Error).message);
      setSessionState("idle");
    }
  }, []);

  const statusMessage =
    sessionState === "capturing"
      ? "Capture is on — your activity is being recorded."
      : sessionState === "processing"
        ? "Wrapping up: ingesting, stitching, and summarizing…"
        : "Idle — start a session when you’re ready to capture again.";

  return (
    <main className="layout">
      <aside className="panel panel--sidebar">
        <div className="panel__header">
          <div className="brand">
            <span className="brand__name">Omega</span>
            <h2 className="brand__title">Sessions</h2>
          </div>
          <button type="button" className="btn-ghost btn-small" onClick={() => void refreshSessions()}>
            Refresh
          </button>
        </div>
        {loading ? <p className="loading-hint">Loading sessions…</p> : null}
        {error ? <p className="error">{error}</p> : null}
        <div className="list">
          {sessions.length === 0 && !loading ? (
            <p className="empty-hint">No sessions yet. End a capture run to see it here.</p>
          ) : null}
          {sessions.map((s) => (
            <button
              type="button"
              key={s.session_key}
              className={`list-item buttonish ${selected?.session_key === s.session_key ? "active" : ""}`}
              onClick={() => setSelected(s)}
            >
              <div>
                <span className="session-key">{s.session_key}</span>
                <div className="muted">
                  {s.accepted_captures} captures · {Math.floor(s.duration_secs / 60)}m
                </div>
              </div>
            </button>
          ))}
        </div>
      </aside>

      <section className="content">
        <section className="panel">
          <div className="row row--top">
            <div>
              <h2 className="session-headline">Live session</h2>
              <div className="session-status-row">
                <span className={statusDotClass(sessionState)} aria-hidden />
                <p className="status-text">{statusMessage}</p>
              </div>
            </div>
            <div className="row gap">
              <button type="button" onClick={() => void handleStartSession()} disabled={sessionState !== "idle"}>
                Start session
              </button>
              <button
                type="button"
                className="primary-button"
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
            <p className="empty-hint">Pick a session from the list to load its summary.</p>
          </section>
        ) : (summary.buckets ?? []).length > 0 ? (
          <BucketSummaryWorkspace
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
