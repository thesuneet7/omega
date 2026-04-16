import { useCallback, useEffect, useState } from "react";
import {
  ACTION_TYPES,
  listActionOutputs,
  runAction,
  type ActionOutputRecord,
  type ActionTypeId,
  type SessionBucket,
} from "../lib/api";

type Props = {
  sessionKey: string;
  buckets: SessionBucket[];
  selectedBucketIds: Set<number>;
};

function formatDate(epochSecs: number): string {
  return new Date(epochSecs * 1000).toLocaleString(undefined, {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function actionLabel(type: string): string {
  return ACTION_TYPES.find((a) => a.id === type)?.label ?? type;
}

export function ActionPanel({ sessionKey, buckets, selectedBucketIds }: Props) {
  const [outputs, setOutputs] = useState<ActionOutputRecord[]>([]);
  const [running, setRunning] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [viewing, setViewing] = useState<ActionOutputRecord | null>(null);

  const refresh = useCallback(async () => {
    try {
      const data = await listActionOutputs(sessionKey);
      setOutputs(data);
    } catch {
      /* ignore background refresh errors */
    }
  }, [sessionKey]);

  useEffect(() => {
    void refresh();
    setViewing(null);
    setError(null);
  }, [refresh]);

  const handleRun = async (actionType: ActionTypeId) => {
    if (running) return;
    setRunning(actionType);
    setError(null);
    try {
      const ids = selectedBucketIds.size > 0 ? [...selectedBucketIds] : undefined;
      const result = await runAction(sessionKey, actionType, ids);
      setOutputs((prev) => [result, ...prev]);
      setViewing(result);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setRunning(null);
    }
  };

  const noneSelected = selectedBucketIds.size === 0;
  const selectionLabel = noneSelected
    ? "all buckets"
    : `${selectedBucketIds.size} bucket${selectedBucketIds.size === 1 ? "" : "s"}`;

  if (viewing) {
    return (
      <section className="panel action-panel">
        <div className="action-panel__toolbar">
          <button
            type="button"
            className="btn-ghost btn-small"
            onClick={() => setViewing(null)}
          >
            ← Back
          </button>
          <span className="action-panel__view-meta">
            {actionLabel(viewing.action_type)} · {formatDate(viewing.generated_at_epoch_secs)}
          </span>
        </div>
        <div className="action-panel__output-body">
          <ActionMarkdown text={viewing.output_body} />
        </div>
      </section>
    );
  }

  return (
    <section className="panel action-panel">
      <h2 className="section-title">Actions</h2>
      <p className="muted action-panel__hint">
        Generate from {selectionLabel}.{noneSelected ? " Select cards above to narrow scope." : ""}
      </p>
      {error ? <p className="error">{error}</p> : null}

      <div className="action-panel__buttons">
        {ACTION_TYPES.map((a) => (
          <button
            key={a.id}
            type="button"
            className="action-btn"
            disabled={running !== null || buckets.length === 0}
            onClick={() => void handleRun(a.id)}
          >
            {running === a.id ? `Generating…` : a.label}
          </button>
        ))}
      </div>

      {outputs.length > 0 ? (
        <div className="action-panel__history">
          <h3 className="action-panel__history-title">Past outputs</h3>
          <div className="action-panel__history-list">
            {outputs.map((o) => (
              <button
                key={o.id}
                type="button"
                className="action-history-item"
                onClick={() => setViewing(o)}
              >
                <span className="action-history-item__type">
                  {actionLabel(o.action_type)}
                </span>
                <span className="action-history-item__date">
                  {formatDate(o.generated_at_epoch_secs)}
                </span>
              </button>
            ))}
          </div>
        </div>
      ) : null}
    </section>
  );
}

/** Simple markdown-to-HTML renderer for action output (handles headings, bold, bullets, paragraphs). */
function ActionMarkdown({ text }: { text: string }) {
  const html = markdownToHtml(text);
  return (
    <div
      className="action-markdown"
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

function markdownToHtml(md: string): string {
  const escaped = md
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");

  const lines = escaped.split("\n");
  const out: string[] = [];
  let inList = false;

  for (const line of lines) {
    const trimmed = line.trimStart();

    if (/^#{1,4}\s/.test(trimmed)) {
      if (inList) { out.push("</ul>"); inList = false; }
      const level = trimmed.match(/^(#{1,4})/)?.[1].length ?? 1;
      const content = trimmed.replace(/^#{1,4}\s+/, "");
      out.push(`<h${level + 1}>${applyInline(content)}</h${level + 1}>`);
    } else if (/^[-*]\s/.test(trimmed)) {
      if (!inList) { out.push("<ul>"); inList = true; }
      const content = trimmed.replace(/^[-*]\s+/, "");
      out.push(`<li>${applyInline(content)}</li>`);
    } else if (/^\d+\.\s/.test(trimmed)) {
      if (inList) { out.push("</ul>"); inList = false; }
      const content = trimmed.replace(/^\d+\.\s+/, "");
      out.push(`<p>${applyInline(content)}</p>`);
    } else if (trimmed === "") {
      if (inList) { out.push("</ul>"); inList = false; }
    } else {
      if (inList) { out.push("</ul>"); inList = false; }
      out.push(`<p>${applyInline(trimmed)}</p>`);
    }
  }
  if (inList) out.push("</ul>");
  return out.join("\n");
}

function applyInline(text: string): string {
  return text
    .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
    .replace(/\*(.+?)\*/g, "<em>$1</em>")
    .replace(/`(.+?)`/g, "<code>$1</code>");
}
