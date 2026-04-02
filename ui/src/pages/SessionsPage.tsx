import { useCallback, useEffect, useMemo, useState } from "react";
import { RevisionHistory } from "../components/RevisionHistory";
import { SummaryEditor } from "../components/SummaryEditor";
import {
  getSessionSummary,
  listPipelineRuns,
  listSessions,
  runPipelineStage,
  saveSummaryRevision,
  type PipelineRunRecord,
  type SessionListItem,
  type SessionSummaryState,
  type SummaryRevision
} from "../lib/api";

export function SessionsPage() {
  const [sessions, setSessions] = useState<SessionListItem[]>([]);
  const [selected, setSelected] = useState<SessionListItem | null>(null);
  const [summary, setSummary] = useState<SessionSummaryState | null>(null);
  const [runs, setRuns] = useState<PipelineRunRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [activityDetected, setActivityDetected] = useState(false);

  const refreshRuns = useCallback(async () => {
    try {
      const data = await listPipelineRuns();
      setRuns(data);
    } catch {
      // ignore when empty or missing table
    }
  }, []);

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
    void refreshRuns();
  }, [refreshSessions, refreshRuns]);

  useEffect(() => {
    const onActivity = () => {
      setActivityDetected(true);
      void refreshSessions();
      void refreshRuns();
    };

    const events: Array<keyof WindowEventMap> = ["click", "keydown", "scroll"];
    events.forEach((evt) => window.addEventListener(evt, onActivity, { once: true }));
    return () => {
      events.forEach((evt) => window.removeEventListener(evt, onActivity));
    };
  }, [refreshSessions, refreshRuns]);

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
      if (!selected) {
        return;
      }
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
      if (!selected) {
        return;
      }
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

  const sourceBuckets = useMemo(
    () => summary?.source_bucket_ids.map((x) => `#${x}`).join(", ") ?? "none",
    [summary]
  );

  const runStage = async (stage: "phase2" | "phase3" | "phase4") => {
    if (stage === "phase2" && !selected) {
      return;
    }
    const inputRef = stage === "phase2" ? selected?.file_path : undefined;
    await runPipelineStage(stage, inputRef);
    await refreshRuns();
    if (selected) {
      const updated = await getSessionSummary(selected.session_key);
      setSummary(updated);
    }
  };

  return (
    <main className="layout">
      <aside className="panel">
        <div className="row">
          <h2>Sessions</h2>
          <button onClick={() => void refreshSessions()}>Refresh</button>
        </div>
        {loading ? <p>Loading sessions...</p> : null}
        {error ? <p className="error">{error}</p> : null}
        <div className="list">
          {sessions.map((s) => (
            <button
              key={s.session_key}
              className={`list-item buttonish ${selected?.session_key === s.session_key ? "active" : ""}`}
              onClick={() => setSelected(s)}
            >
              <div>
                <strong>{s.session_key}</strong>
                <div className="muted">
                  {s.accepted_captures} captures, {Math.floor(s.duration_secs / 60)}m
                </div>
              </div>
            </button>
          ))}
        </div>
      </aside>

      <section className="content">
        <section className="panel">
          <div className="row">
            <h2>Pipeline</h2>
            <div className="row gap">
              <button onClick={() => void runStage("phase2")}>Run Phase2</button>
              <button onClick={() => void runStage("phase3")}>Run Phase3</button>
              <button onClick={() => void runStage("phase4")}>Run Phase4</button>
            </div>
          </div>
          <p className="muted">
            Auto mode: {activityDetected ? "active (interaction detected)" : "waiting for interaction"}
          </p>
          <p className="muted">Source buckets: {sourceBuckets}</p>
          <div className="list">
            {runs.map((r) => (
              <div className="list-item" key={r.id}>
                <span>
                  {r.stage} - {r.status}
                </span>
                <span className="muted">{new Date(r.started_at_epoch_secs * 1000).toLocaleString()}</span>
              </div>
            ))}
          </div>
        </section>

        {summary ? (
          <SummaryEditor
            initialTitle={summary.title}
            initialBody={summary.body}
            onSave={handleSave}
            saving={saving}
          />
        ) : (
          <section className="panel">
            <p>Select a session to open summary.</p>
          </section>
        )}

        <RevisionHistory revisions={summary?.revisions ?? []} onRestore={handleRestore} />
      </section>
    </main>
  );
}
