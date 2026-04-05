import type { SummaryRevision } from "../lib/api";

type Props = {
  revisions: SummaryRevision[];
  onRestore: (rev: SummaryRevision) => void;
};

export function RevisionHistory({ revisions, onRestore }: Props) {
  return (
    <section className="panel">
      <h2 className="section-title">Revision history</h2>
      {revisions.length === 0 ? (
        <p className="empty-hint empty-hint--below-title">No revisions yet — edits will appear here.</p>
      ) : (
        <div className="list">
          {revisions.map((rev) => (
            <div className="list-item" key={rev.id}>
              <div>
                <strong className="session-key">{rev.title}</strong>
                <div className="revision-meta">
                  {new Date(rev.edited_at_epoch_secs * 1000).toLocaleString()} · {rev.editor_label}
                </div>
              </div>
              <button type="button" className="btn-small btn-restore" onClick={() => onRestore(rev)}>
                Restore
              </button>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
