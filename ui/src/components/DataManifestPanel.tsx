import { useCallback, useEffect, useState } from "react";
import {
  deleteAllLocalSessionData,
  deleteSessionData,
  getStorageManifest,
  type StorageManifest,
} from "../lib/api";

const CATEGORY_LABEL: Record<string, string> = {
  capture_session: "Capture log",
  app_database: "App database",
  phase_database: "Research DB (embeddings)",
  privacy_config: "Privacy settings file",
  live_status: "Live capture status",
};

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

type Props = {
  selectedSessionKey: string | null;
  onDataChanged: () => void;
};

export function DataManifestPanel({ selectedSessionKey, onDataChanged }: Props) {
  const [manifest, setManifest] = useState<StorageManifest | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const m = await getStorageManifest();
      setManifest(m);
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleDeleteSession = useCallback(async () => {
    if (!selectedSessionKey) return;
    if (
      !window.confirm(
        "Remove this session’s capture log and saved summary data from disk? This cannot be undone."
      )
    ) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const r = await deleteSessionData(selectedSessionKey);
      if (r.message) {
        window.alert(r.message);
      }
      onDataChanged();
      await refresh();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }, [selectedSessionKey, onDataChanged, refresh]);

  const handleDeleteAll = useCallback(async () => {
    if (
      !window.confirm(
        "Delete ALL local Omega data in the logs folder (captures, summaries, databases)? Your app exclusion list will be kept. Restart the app before running ingest or summarize again. This cannot be undone."
      )
    ) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const r = await deleteAllLocalSessionData();
      window.alert(r.message);
      onDataChanged();
      await refresh();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }, [onDataChanged, refresh]);

  return (
    <section className="data-manifest" aria-labelledby="data-manifest-title">
      <h3 id="data-manifest-title" className="data-manifest__title">
        Stored data
      </h3>
      <p className="data-manifest__root" title={manifest?.logsRootAbsolute}>
        <span className="muted">Folder:</span>{" "}
        <code className="data-manifest__path">{manifest?.logsRootAbsolute ?? "…"}</code>
      </p>
      {manifest ? (
        <p className="data-manifest__hint">{manifest.retentionNote}</p>
      ) : null}
      {loading ? <p className="muted data-manifest__status">Loading…</p> : null}
      {error ? <p className="error data-manifest__error">{error}</p> : null}
      {manifest && !loading ? (
        <>
          <p className="data-manifest__total">
            <span className="muted">Total on disk (known files):</span>{" "}
            <strong>{formatBytes(manifest.totalBytes)}</strong>
            {manifest.entries.length > 0 ? (
              <span className="muted"> · {manifest.entries.length} files</span>
            ) : null}
          </p>
          {manifest.entries.length === 0 ? (
            <p className="muted data-manifest__empty">No tracked files in this folder yet.</p>
          ) : (
            <details className="data-manifest__details">
              <summary>Browse files on disk</summary>
              <ul className="data-manifest__list" aria-label="Stored files">
                {manifest.entries.map((e) => (
                  <li key={e.absolutePath} className="data-manifest__item">
                    <div className="data-manifest__item-head">
                      <span className="data-manifest__cat">
                        {CATEGORY_LABEL[e.category] ?? e.category}
                      </span>
                      <span className="data-manifest__size">{formatBytes(e.bytes)}</span>
                    </div>
                    <code className="data-manifest__file" title={e.absolutePath}>
                      {e.path}
                    </code>
                  </li>
                ))}
              </ul>
            </details>
          )}
        </>
      ) : null}
      <div className="data-manifest__actions">
        <button
          type="button"
          className="btn-small"
          disabled={busy || !selectedSessionKey}
          onClick={() => void handleDeleteSession()}
        >
          Delete selected session
        </button>
        <button
          type="button"
          className="btn-ghost btn-small data-manifest__danger"
          disabled={busy}
          onClick={() => void handleDeleteAll()}
        >
          Delete all local data
        </button>
        <button type="button" className="btn-ghost btn-small" disabled={busy || loading} onClick={() => void refresh()}>
          Refresh
        </button>
      </div>
    </section>
  );
}
