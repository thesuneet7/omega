import { useCallback, useEffect, useState } from "react";
import { getCaptureExclusions, setCaptureExclusions } from "../lib/api";

export function ExcludedAppsPanel() {
  const [names, setNames] = useState<string[]>([]);
  const [draft, setDraft] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const data = await getCaptureExclusions();
      setNames(data.excludedAppNames);
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

  const persist = useCallback(async (next: string[]) => {
    setSaving(true);
    try {
      const data = await setCaptureExclusions(next);
      setNames(data.excludedAppNames);
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSaving(false);
    }
  }, []);

  const handleAdd = useCallback(() => {
    const t = draft.trim();
    if (!t) return;
    setDraft("");
    const lower = new Set(names.map((n) => n.toLowerCase()));
    if (lower.has(t.toLowerCase())) return;
    void persist([...names, t]);
  }, [draft, names, persist]);

  const handleRemove = useCallback(
    (idx: number) => {
      const next = names.filter((_, i) => i !== idx);
      void persist(next);
    },
    [names, persist]
  );

  return (
    <section className="exclusion-panel" aria-labelledby="exclusion-panel-title">
      <h3 id="exclusion-panel-title" className="exclusion-panel__title">
        Excluded apps
      </h3>
      <p className="exclusion-panel__hint">
        Omega will not capture screenshots while one of these apps is frontmost. Use the exact name shown in the
        macOS menu bar (e.g. <span className="exclusion-panel__mono">Safari</span>,{" "}
        <span className="exclusion-panel__mono">Google Chrome</span>).
      </p>
      {loading ? <p className="muted exclusion-panel__status">Loading…</p> : null}
      {error ? <p className="error exclusion-panel__error">{error}</p> : null}
      <div className="exclusion-panel__add">
        <input
          type="text"
          className="exclusion-panel__input"
          placeholder="App name"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void handleAdd();
            }
          }}
          disabled={saving || loading}
          aria-label="App name to exclude from capture"
        />
        <button type="button" className="btn-small" onClick={() => void handleAdd()} disabled={saving || loading}>
          Add
        </button>
      </div>
      {names.length === 0 && !loading ? (
        <p className="muted exclusion-panel__empty">No apps excluded yet.</p>
      ) : (
        <ul className="exclusion-panel__list">
          {names.map((n, i) => (
            <li key={`${n}-${i}`} className="exclusion-panel__item">
              <span className="exclusion-panel__name">{n}</span>
              <button
                type="button"
                className="btn-ghost btn-small"
                onClick={() => handleRemove(i)}
                disabled={saving}
                aria-label={`Remove ${n} from exclusions`}
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
      {saving ? <p className="muted exclusion-panel__status">Saving…</p> : null}
    </section>
  );
}
