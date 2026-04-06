import type { SummaryRevision } from "../lib/api";
import { formatEditorLabel, formatRevisionTimestamp, humanizeSummaryTitle } from "../lib/sessionDisplay";

type Props = {
  revisions: SummaryRevision[];
  onRestore: (rev: SummaryRevision) => void;
};

export function RevisionHistory({ revisions, onRestore }: Props) {
  return (
    <section className="panel">
      <h2 className="section-title">Past versions</h2>
      {revisions.length === 0 ? (
        <p className="empty-hint empty-hint--below-title">No saved versions yet — edits you save will show up here.</p>
      ) : (
        <div className="list">
          {revisions.map((rev) => (
            <div className="list-item" key={rev.id}>
              <div>
                <strong className="revision-row__title">{humanizeSummaryTitle(rev.title)}</strong>
                <div className="revision-meta">
                  {formatRevisionTimestamp(rev.edited_at_epoch_secs)} · {formatEditorLabel(rev.editor_label)}
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
