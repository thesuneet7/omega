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
  if (type === "custom") return "Custom";
  return ACTION_TYPES.find((a) => a.id === type)?.label ?? type;
}

export function ActionPanel({ sessionKey, buckets, selectedBucketIds }: Props) {
  const [outputs, setOutputs] = useState<ActionOutputRecord[]>([]);
  const [running, setRunning] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [viewing, setViewing] = useState<ActionOutputRecord | null>(null);
  const [customPrompt, setCustomPrompt] = useState("");
  const [briefWindow, setBriefWindow] = useState<"session" | "last24h" | "thisWeek">("session");
  const [hideLowConfidence, setHideLowConfidence] = useState(false);
  const [showEvidencedOnly, setShowEvidencedOnly] = useState(false);

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
    setHideLowConfidence(false);
    setShowEvidencedOnly(false);
  }, [refresh]);

  const handleRun = async (actionType: ActionTypeId, prompt?: string) => {
    if (running) return;
    setRunning(actionType);
    setError(null);
    try {
      const ids = selectedBucketIds.size > 0 ? [...selectedBucketIds] : undefined;
      const stakeholderPrompt =
        actionType === "stakeholder_brief"
          ? briefWindow === "last24h"
            ? "Use the last 24 hours of captured context."
            : briefWindow === "thisWeek"
              ? "Use this week of captured context."
              : "Use the selected session window."
          : prompt;
      const result = await runAction(sessionKey, actionType, ids, stakeholderPrompt);
      setOutputs((prev) => [result, ...prev]);
      setViewing(result);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setRunning(null);
    }
  };

  const handleCustomRun = () => {
    if (!customPrompt.trim()) return;
    void handleRun("custom", customPrompt.trim());
  };

  const noneSelected = selectedBucketIds.size === 0;
  const selectionLabel = noneSelected
    ? "all buckets"
    : `${selectedBucketIds.size} bucket${selectedBucketIds.size === 1 ? "" : "s"}`;

  if (viewing) {
    const evidenceItems = parseEvidenceAppendix(viewing.output_body);
    const evidenceIdSet = new Set(evidenceItems.map((item) => item.id));
    const visibleEvidenceItems = hideLowConfidence
      ? evidenceItems.filter((item) => !item.confidence.toLowerCase().includes("low"))
      : evidenceItems;
    const filtered = filterMarkdownForTrust(viewing.output_body, {
      hideLowConfidence,
      showEvidencedOnly,
      validEvidenceIds: evidenceIdSet,
    });
    const danglingCitationCount = countDanglingCitations(viewing.output_body, evidenceIdSet);
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
          {danglingCitationCount > 0 ? (
            <p className="action-panel__trust-warning">
              Found {danglingCitationCount} dangling citation
              {danglingCitationCount === 1 ? "" : "s"} that do not resolve to evidence.
            </p>
          ) : null}
          <div className="action-panel__trust-controls">
            <label className="action-panel__toggle">
              <input
                type="checkbox"
                checked={hideLowConfidence}
                onChange={(e) => setHideLowConfidence(e.target.checked)}
              />
              Hide low-confidence claims
            </label>
            <label className="action-panel__toggle">
              <input
                type="checkbox"
                checked={showEvidencedOnly}
                onChange={(e) => setShowEvidencedOnly(e.target.checked)}
              />
              Show only directly evidenced claims
            </label>
          </div>
          <ActionMarkdown text={filtered} validEvidenceIds={evidenceIdSet} />
          <section className="action-evidence-panel">
            <h3 className="action-evidence-panel__title">Evidence panel</h3>
            {visibleEvidenceItems.length > 0 ? (
              <div className="action-evidence-panel__list">
                {visibleEvidenceItems.map((item) => (
                  <article key={item.id} id={`evidence-${item.id}`} className="action-evidence-item">
                    <div className="action-evidence-item__top">
                      <span className="action-evidence-item__badge">[E{item.id}]</span>
                      <span className="action-evidence-item__confidence">{item.confidence}</span>
                    </div>
                    <p><strong>Source app:</strong> {item.sourceApp}</p>
                    <p><strong>Origin:</strong> {item.origin}</p>
                    <p><strong>Timestamp:</strong> {item.timestamp}</p>
                    <p><strong>Snippet:</strong> {item.snippet}</p>
                  </article>
                ))}
              </div>
            ) : (
              <p className="action-evidence-panel__empty">
                {evidenceItems.length > 0 && hideLowConfidence
                  ? "All evidence items are low confidence and currently hidden."
                  : "No evidence references found in this output. Generate again to produce evidence-locked claims."}
              </p>
            )}
          </section>
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
            className={`action-btn${a.id === "stakeholder_brief" ? " action-btn--primary" : ""}`}
            disabled={running !== null || buckets.length === 0}
            onClick={() => void handleRun(a.id)}
          >
            {running === a.id ? `Generating…` : a.label}
          </button>
        ))}
      </div>
      <div className="action-panel__window">
        <label className="action-panel__window-label" htmlFor="brief-window">
          Stakeholder brief time window
        </label>
        <select
          id="brief-window"
          className="action-panel__window-select"
          value={briefWindow}
          onChange={(e) => setBriefWindow(e.target.value as "session" | "last24h" | "thisWeek")}
          disabled={running !== null || buckets.length === 0}
        >
          <option value="session">Selected session</option>
          <option value="last24h">Last 24 hours</option>
          <option value="thisWeek">This week</option>
        </select>
      </div>

      <div className="action-panel__custom">
        <div className="action-panel__custom-row">
          <input
            type="text"
            className="action-panel__custom-input"
            placeholder="Enter a custom prompt… e.g. &quot;Write a Slack standup update&quot;"
            value={customPrompt}
            onChange={(e) => setCustomPrompt(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                handleCustomRun();
              }
            }}
            disabled={running !== null || buckets.length === 0}
          />
          <button
            type="button"
            className="action-btn action-btn--custom"
            disabled={running !== null || buckets.length === 0 || !customPrompt.trim()}
            onClick={handleCustomRun}
          >
            {running === "custom" ? "Generating…" : "Generate"}
          </button>
        </div>
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
function ActionMarkdown({ text, validEvidenceIds }: { text: string; validEvidenceIds: Set<number> }) {
  const html = markdownToHtml(text, validEvidenceIds);
  return (
    <div
      className="action-markdown"
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

function markdownToHtml(md: string, validEvidenceIds: Set<number>): string {
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
      out.push(`<h${level + 1}>${applyInline(content, validEvidenceIds)}</h${level + 1}>`);
    } else if (/^[-*]\s/.test(trimmed)) {
      if (!inList) { out.push("<ul>"); inList = true; }
      const content = trimmed.replace(/^[-*]\s+/, "");
      out.push(`<li>${applyInline(content, validEvidenceIds)}</li>`);
    } else if (/^\d+\.\s/.test(trimmed)) {
      if (inList) { out.push("</ul>"); inList = false; }
      const content = trimmed.replace(/^\d+\.\s+/, "");
      out.push(`<p>${applyInline(content, validEvidenceIds)}</p>`);
    } else if (trimmed === "") {
      if (inList) { out.push("</ul>"); inList = false; }
    } else {
      if (inList) { out.push("</ul>"); inList = false; }
      out.push(`<p>${applyInline(trimmed, validEvidenceIds)}</p>`);
    }
  }
  if (inList) out.push("</ul>");
  return out.join("\n");
}

function applyInline(text: string, validEvidenceIds: Set<number>): string {
  return text
    .replace(/\[E(\d+)\]/g, (_, idRaw: string) => {
      const id = Number(idRaw);
      const isValid = validEvidenceIds.has(id);
      const cls = isValid ? "action-citation" : "action-citation action-citation--dangling";
      return `<a href="#evidence-${id}" class="${cls}">[E${id}]</a>`;
    })
    .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
    .replace(/\*(.+?)\*/g, "<em>$1</em>")
    .replace(/`(.+?)`/g, "<code>$1</code>");
}

type EvidenceItem = {
  id: number;
  sourceApp: string;
  origin: string;
  timestamp: string;
  snippet: string;
  confidence: string;
};

function parseEvidenceAppendix(markdown: string): EvidenceItem[] {
  const appendixIdx = markdown.indexOf("## Evidence Appendix");
  if (appendixIdx < 0) return [];
  const lines = markdown.slice(appendixIdx).split("\n");
  const out: EvidenceItem[] = [];
  let current: EvidenceItem | null = null;

  for (const line of lines) {
    const head = line.match(/^- \[E(\d+)\] source app:\s*(.+)$/);
    if (head) {
      if (current) out.push(current);
      current = {
        id: Number(head[1]),
        sourceApp: head[2].trim(),
        origin: "unknown-origin",
        timestamp: "unknown",
        snippet: "n/a",
        confidence: "Medium",
      };
      continue;
    }
    if (!current) continue;
    const t = line.trim();
    if (t.startsWith("- origin:")) current.origin = t.replace("- origin:", "").trim();
    else if (t.startsWith("- timestamp:")) current.timestamp = t.replace("- timestamp:", "").trim();
    else if (t.startsWith("- snippet:")) current.snippet = t.replace("- snippet:", "").trim();
    else if (t.startsWith("- confidence:")) current.confidence = t.replace("- confidence:", "").trim();
  }
  if (current) out.push(current);
  return out;
}

function filterMarkdownForTrust(
  markdown: string,
  opts: { hideLowConfidence: boolean; showEvidencedOnly: boolean; validEvidenceIds: Set<number> }
): string {
  const appendixIdx = markdown.indexOf("## Evidence Appendix");
  const main = appendixIdx >= 0 ? markdown.slice(0, appendixIdx) : markdown;
  const appendix = appendixIdx >= 0 ? markdown.slice(appendixIdx) : "";
  const filtered = main
    .split("\n")
    .filter((line) => {
      const t = line.trim();
      if (t === "" || t.startsWith("#")) return true;
      if (opts.hideLowConfidence && /\[low\]/i.test(t)) return false;
      const ids = extractEvidenceIdsFromLine(t);
      if (opts.showEvidencedOnly) {
        if (ids.length === 0) return false;
        if (ids.every((id) => !opts.validEvidenceIds.has(id))) return false;
      }
      return true;
    })
    .join("\n")
    .trimEnd();
  if (!appendix) return filtered;
  return `${filtered}\n\n${appendix.trim()}`;
}

function extractEvidenceIdsFromLine(line: string): number[] {
  const out: number[] = [];
  const matches = line.matchAll(/\[E(\d+)\]/g);
  for (const m of matches) {
    const n = Number(m[1]);
    if (Number.isFinite(n)) out.push(n);
  }
  return out;
}

function countDanglingCitations(markdown: string, validEvidenceIds: Set<number>): number {
  const appendixIdx = markdown.indexOf("## Evidence Appendix");
  const main = appendixIdx >= 0 ? markdown.slice(0, appendixIdx) : markdown;
  let count = 0;
  for (const line of main.split("\n")) {
    for (const id of extractEvidenceIdsFromLine(line)) {
      if (!validEvidenceIds.has(id)) count += 1;
    }
  }
  return count;
}
