import { useCallback, useEffect, useRef, useState } from "react";
import { RevisionHistory } from "./RevisionHistory";
import { encodeBucketStorage, type SessionBucket, type SummaryRevision } from "../lib/api";
import { humanizeSummaryTitle } from "../lib/sessionDisplay";

type Props = {
  sessionTitle: string;
  buckets: SessionBucket[];
  revisions: SummaryRevision[];
  saving: boolean;
  onSave: (sessionTitle: string, storageBody: string, autosave: boolean) => Promise<void>;
  onRestore: (rev: SummaryRevision) => void;
};

function excerpt(text: string, max = 140): string {
  const t = text.trim().replace(/\s+/g, " ");
  if (t.length <= max) return t;
  return `${t.slice(0, max)}…`;
}

export function BucketSummaryWorkspace({
  sessionTitle,
  buckets: initialBuckets,
  revisions,
  saving,
  onSave,
  onRestore,
}: Props) {
  const [view, setView] = useState<"home" | "editor">("home");
  const [activeIndex, setActiveIndex] = useState(0);
  const [buckets, setBuckets] = useState<SessionBucket[]>(initialBuckets);
  const [bt, setBt] = useState("");
  const [bb, setBb] = useState("");
  const [dirty, setDirty] = useState(false);
  const bucketsSigRef = useRef<string | null>(null);

  useEffect(() => {
    const sig = JSON.stringify(initialBuckets);
    if (bucketsSigRef.current === sig) return;
    bucketsSigRef.current = sig;
    setBuckets(initialBuckets);
    setView("home");
  }, [initialBuckets]);

  const flushSave = useCallback(
    async (next: SessionBucket[], autosave: boolean) => {
      await onSave(sessionTitle, encodeBucketStorage(next), autosave);
      setBuckets(next);
    },
    [onSave, sessionTitle]
  );

  useEffect(() => {
    if (!dirty || view !== "editor") return;
    const t = setTimeout(() => {
      const next = buckets.map((b, i) => (i === activeIndex ? { ...b, title: bt, body: bb } : b));
      void flushSave(next, true);
      setDirty(false);
    }, 1200);
    return () => clearTimeout(t);
  }, [dirty, bt, bb, activeIndex, view, buckets, flushSave]);

  const openBucket = (index: number) => {
    const b = buckets[index];
    if (!b) return;
    setActiveIndex(index);
    setBt(b.title);
    setBb(b.body);
    setDirty(false);
    setView("editor");
  };

  const handleBack = () => {
    if (view === "editor") {
      const next = buckets.map((b, i) => (i === activeIndex ? { ...b, title: bt, body: bb } : b));
      setBuckets(next);
      if (dirty) {
        void flushSave(next, true);
      }
      setDirty(false);
      setView("home");
    }
  };

  const handleManualSave = () => {
    const next = buckets.map((b, i) => (i === activeIndex ? { ...b, title: bt, body: bb } : b));
    void flushSave(next, false);
    setDirty(false);
  };

  if (view === "home") {
    return (
      <>
        <section className="panel">
          <h2 className="section-title">Session summary</h2>
          <p className="bucket-session-title">{humanizeSummaryTitle(sessionTitle)}</p>
          <p className="muted bucket-hint">Open a card to read and edit. Your work saves automatically.</p>
          <div className="bucket-grid">
            {buckets.map((b, i) => (
              <button
                type="button"
                key={`${b.bucket_id}-${i}`}
                className="bucket-card"
                onClick={() => openBucket(i)}
              >
                <span className="bucket-card__title">{b.title || "Untitled"}</span>
                <span className="bucket-card__excerpt">{excerpt(b.body) || "No content yet."}</span>
              </button>
            ))}
          </div>
        </section>
        <RevisionHistory revisions={revisions} onRestore={onRestore} />
      </>
    );
  }

  return (
    <section className="panel bucket-editor-panel">
      <div className="bucket-editor-toolbar">
        <button type="button" className="btn-ghost btn-small" onClick={handleBack}>
          ← Back to overview
        </button>
        <button type="button" disabled={saving} onClick={handleManualSave}>
          {saving ? "Saving…" : "Save revision"}
        </button>
      </div>
      <input
        className="input-title"
        value={bt}
        onChange={(e) => {
          setBt(e.target.value);
          setDirty(true);
        }}
        placeholder="Title"
        aria-label="Bucket title"
      />
      <textarea
        className="editor-body"
        value={bb}
        onChange={(e) => {
          setBb(e.target.value);
          setDirty(true);
        }}
        rows={18}
        placeholder="Summary for this theme…"
        aria-label="Bucket body"
      />
    </section>
  );
}
