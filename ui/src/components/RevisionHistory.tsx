import type { SummaryRevision } from "../lib/api";

type Props = {
  revisions: SummaryRevision[];
  onRestore: (rev: SummaryRevision) => void;
};

export function RevisionHistory({ revisions, onRestore }: Props) {
  return (
    <section className="panel">
      <h2>Revision History</h2>
      <div className="list">
        {revisions.map((rev) => (
          <div className="list-item" key={rev.id}>
            <div>
              <strong>{rev.title}</strong>
              <div className="muted">
                {new Date(rev.edited_at_epoch_secs * 1000).toLocaleString()} by {rev.editor_label}
              </div>
            </div>
            <button onClick={() => onRestore(rev)}>Restore</button>
          </div>
        ))}
      </div>
    </section>
  );
}
