import { useCallback, useEffect, useRef, useState } from "react";
import { ActionPanel } from "./ActionPanel";
import { NotionLikeMarkdownEditor } from "./NotionLikeMarkdownEditor";
import { RevisionHistory } from "./RevisionHistory";
import {
  encodeBucketStorage,
  type SessionBucket,
  type SourceAttribution,
  type SummaryRevision,
} from "../lib/api";
import { humanizeSummaryTitle } from "../lib/sessionDisplay";

type Props = {
  sessionKey: string;
  sessionTitle: string;
  buckets: SessionBucket[];
  revisions: SummaryRevision[];
  saving: boolean;
  onSave: (sessionTitle: string, storageBody: string, autosave: boolean) => Promise<void>;
  onRestore: (rev: SummaryRevision) => void;
};

function bodyPreviewWithoutSourcesBlock(body: string): string {
  const marker = "**Sources (from capture metadata):**";
  const idx = body.indexOf(marker);
  if (idx < 0) return body;
  return body.slice(0, idx).trimEnd();
}

function excerpt(text: string, max = 140): string {
  const t = text.trim().replace(/\s+/g, " ");
  if (t.length <= max) return t;
  return `${t.slice(0, max)}…`;
}

function formatSourceLabel(s: SourceAttribution): string {
  const w = s.window_title.trim();
  if (!w) return s.app_name;
  return `${s.app_name} — ${w}`;
}

function formatSourcesForCard(sources: SourceAttribution[] | undefined, max = 120): string {
  if (!sources?.length) return "";
  const line = sources.map(formatSourceLabel).join("; ");
  if (line.length <= max) return line;
  return `${line.slice(0, max)}…`;
}

export function BucketSummaryWorkspace({
  sessionKey,
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
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());

  useEffect(() => {
    const sig = JSON.stringify(initialBuckets);
    if (bucketsSigRef.current === sig) return;
    bucketsSigRef.current = sig;
    setBuckets(initialBuckets);
    setSelectedIds(new Set());
    setView("home");
  }, [initialBuckets]);

  const toggleSelect = (bucketId: number) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(bucketId)) next.delete(bucketId);
      else next.add(bucketId);
      return next;
    });
  };

  const toggleSelectAll = () => {
    if (selectedIds.size === buckets.length) {
      setSelectedIds(new Set());
    } else {
      setSelectedIds(new Set(buckets.map((b) => b.bucket_id)));
    }
  };

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

  const allSelected = selectedIds.size === buckets.length && buckets.length > 0;
  const someSelected = selectedIds.size > 0;

  if (view === "home") {
    return (
      <>
        <section className="panel">
          <div className="bucket-header">
            <div>
              <h2 className="section-title">Session summary</h2>
              <p className="bucket-session-title">{humanizeSummaryTitle(sessionTitle)}</p>
            </div>
            {buckets.length > 0 && (
              <button type="button" className="btn-ghost btn-small" onClick={toggleSelectAll}>
                {allSelected ? "Deselect all" : "Select all"}
              </button>
            )}
          </div>
          {someSelected && (
            <p className="bucket-selection-hint">
              {selectedIds.size} of {buckets.length} selected
            </p>
          )}
          {!someSelected && (
            <p className="muted bucket-hint">Select cards below, then generate a document. Click a card title to edit.</p>
          )}
          <div className="bucket-grid">
            {buckets.map((b, i) => {
              const isSelected = selectedIds.has(b.bucket_id);
              return (
                <div
                  key={`${b.bucket_id}-${i}`}
                  className={`bucket-card${isSelected ? " bucket-card--selected" : ""}`}
                >
                  <label className="bucket-card__check" onClick={(e) => e.stopPropagation()}>
                    <input
                      type="checkbox"
                      checked={isSelected}
                      onChange={() => toggleSelect(b.bucket_id)}
                    />
                    <span className="bucket-card__checkmark" />
                  </label>
                  <button
                    type="button"
                    className="bucket-card__body"
                    onClick={() => openBucket(i)}
                  >
                    <span className="bucket-card__title">{b.title || "Untitled"}</span>
                    <span className="bucket-card__excerpt">
                      {excerpt(bodyPreviewWithoutSourcesBlock(b.body)) || "No content yet."}
                    </span>
                    {b.source_attribution && b.source_attribution.length > 0 ? (
                      <span className="bucket-card__sources" title={b.source_attribution.map(formatSourceLabel).join("; ")}>
                        {formatSourcesForCard(b.source_attribution)}
                      </span>
                    ) : null}
                  </button>
                </div>
              );
            })}
          </div>
        </section>
        <ActionPanel sessionKey={sessionKey} buckets={buckets} selectedBucketIds={selectedIds} />
        <RevisionHistory revisions={revisions} onRestore={onRestore} />
      </>
    );
  }

  const activeBucket = buckets[activeIndex];
  const activeSources = activeBucket?.source_attribution;

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
      {activeSources && activeSources.length > 0 ? (
        <div className="bucket-sources-readonly" aria-label="Capture sources for this summary">
          <div className="bucket-sources-readonly__label">Sources (from capture metadata)</div>
          <ul className="bucket-sources-readonly__list">
            {activeSources.map((s, j) => (
              <li key={`${s.app_name}-${s.window_title}-${j}`}>{formatSourceLabel(s)}</li>
            ))}
          </ul>
        </div>
      ) : null}
      <NotionLikeMarkdownEditor
        className="editor-body-notion"
        markdown={bb}
        onMarkdownChange={(md) => {
          setBb(md);
          setDirty(true);
        }}
        placeholder="Summary for this theme… Type / for blocks."
        aria-label="Bucket body"
        documentTitle={bt || activeBucket?.title || "Bucket summary"}
      />
    </section>
  );
}
