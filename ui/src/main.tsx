import React from "react";
import { createRoot } from "react-dom/client";
import { fetchRuntimeSummary } from "./lib/api";
import "./styles.css";

function App() {
  const [entries, setEntries] = React.useState<string[]>([]);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const editorText = React.useMemo(() => {
    if (entries.length === 0) {
      return "";
    }
    return entries
      .map((value, idx) => `Summary ${idx + 1}\n\n${value.trim()}`)
      .join("\n\n----------------------------------------\n\n");
  }, [entries]);

  const onFetch = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await fetchRuntimeSummary();
      setEntries((prev) => [...prev, result.summary]);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <main className="editor-layout">
      <div className="editor-topbar">
        <button className="primary-button" onClick={() => void onFetch()} disabled={busy}>
          {busy ? "fetching..." : "fetch"}
        </button>
        {error ? <span className="error">{error}</span> : null}
      </div>
      <textarea
        className="editor-surface"
        value={editorText}
        readOnly
        placeholder="Runtime summaries will appear here after you press fetch."
      />
    </main>
  );
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
