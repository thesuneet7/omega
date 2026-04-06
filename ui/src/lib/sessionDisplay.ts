import type { SessionListItem } from "./api";

/** e.g. "17th April" (day ordinal + full month; no year in the main label). */
export function formatSessionDateLine(epochSecs: number): string {
  const d = new Date(epochSecs * 1000);
  if (Number.isNaN(d.getTime())) return "";
  const day = d.getDate();
  const month = d.toLocaleString("en-GB", { month: "long" });
  return `${ordinalDay(day)} ${month}`;
}

function ordinalDay(n: number): string {
  if (n >= 11 && n <= 13) return `${n}th`;
  switch (n % 10) {
    case 1:
      return `${n}st`;
    case 2:
      return `${n}nd`;
    case 3:
      return `${n}rd`;
    default:
      return `${n}th`;
  }
}

const DEFAULT_TITLE_PREFIX = /^session summary\s*-\s*/i;
const CAPTURE_SESSION_TOKEN = /capture-session-\d+/i;

/** Strip auto-generated / file-based titles and return a short label for the UI. */
export function humanizeSummaryTitle(raw: string | null | undefined): string {
  const t = raw?.trim();
  if (!t) return "Untitled session";
  if (DEFAULT_TITLE_PREFIX.test(t)) {
    const rest = t.replace(DEFAULT_TITLE_PREFIX, "").trim();
    if (rest && !CAPTURE_SESSION_TOKEN.test(rest)) return rest;
    return "Untitled session";
  }
  if (CAPTURE_SESSION_TOKEN.test(t)) return "Untitled session";
  return t;
}

/** Date + time for revision rows, e.g. "17 April 2026, 15:42". */
export function formatRevisionTimestamp(epochSecs: number): string {
  const d = new Date(epochSecs * 1000);
  if (Number.isNaN(d.getTime())) return "";
  const datePart = d.toLocaleString("en-GB", {
    day: "numeric",
    month: "long",
    year: "numeric",
  });
  const timePart = d.toLocaleString("en-GB", {
    hour: "2-digit",
    minute: "2-digit",
  });
  return `${datePart}, ${timePart}`;
}

/** Turn stored editor labels into readable phrases. */
export function formatEditorLabel(label: string): string {
  const map: Record<string, string> = {
    autosave: "Autosave",
    "manual-save": "Saved",
    "restore-revision": "Restored from history",
    "system-generated": "Generated summary",
    "local-user": "You",
  };
  return map[label] ?? label.replace(/-/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
}

export function sessionListPrimaryLine(
  item: SessionListItem,
  opts?: { selectedSessionKey: string | null; currentSummaryTitle: string | null }
): string {
  let raw: string | undefined;
  if (
    opts?.selectedSessionKey === item.session_key &&
    opts.currentSummaryTitle != null &&
    opts.currentSummaryTitle !== ""
  ) {
    raw = opts.currentSummaryTitle;
  } else {
    raw = item.summary_title ?? undefined;
  }
  const h = humanizeSummaryTitle(raw);
  if (h !== "Untitled session") return h;
  return formatSessionDateLine(item.started_at_epoch_secs) || "Session";
}

/** Second line: date (unless the primary line is already the date), captures, duration. */
export function sessionListSecondaryLine(item: SessionListItem, primaryLine: string): string {
  const dateLine = formatSessionDateLine(item.started_at_epoch_secs);
  const mins = Math.floor(item.duration_secs / 60);
  const parts: string[] = [];
  if (primaryLine !== dateLine && dateLine) {
    parts.push(dateLine);
  }
  parts.push(`${item.accepted_captures} captures`);
  parts.push(mins < 1 ? "under 1 min" : `${mins} min`);
  return parts.join(" · ");
}
