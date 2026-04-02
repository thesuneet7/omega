export type SessionListItem = {
  session_key: string;
  file_path: string;
  started_at_epoch_secs: number;
  ended_at_epoch_secs: number;
  duration_secs: number;
  accepted_captures: number;
  total_events_seen: number;
};

export type SummaryRevision = {
  id: number;
  summary_id: number;
  title: string;
  body: string;
  edited_at_epoch_secs: number;
  editor_label: string;
};

export type SessionSummaryState = {
  session_key: string;
  title: string;
  body: string;
  source_bucket_ids: number[];
  revisions: SummaryRevision[];
};

export type PipelineRunRecord = {
  id: number;
  stage: string;
  input_ref: string;
  status: string;
  started_at_epoch_secs: number;
  ended_at_epoch_secs?: number;
  error_text?: string;
};

function apiBase(): string {
  if (typeof window !== "undefined" && window.omega?.apiBase) {
    return window.omega.apiBase;
  }
  return import.meta.env.VITE_OMEGA_API_URL ?? "http://127.0.0.1:17421";
}

async function apiGet<T>(path: string): Promise<T> {
  const r = await fetch(`${apiBase()}${path}`);
  if (!r.ok) {
    const t = await r.text();
    throw new Error(t || r.statusText);
  }
  return r.json() as Promise<T>;
}

async function apiPost<T>(path: string, body: unknown): Promise<T> {
  const r = await fetch(`${apiBase()}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!r.ok) {
    const t = await r.text();
    throw new Error(t || r.statusText);
  }
  return r.json() as Promise<T>;
}

export async function listSessions(): Promise<SessionListItem[]> {
  return apiGet("/api/sessions");
}

export async function getSessionSummary(sessionKey: string): Promise<SessionSummaryState> {
  const q = new URLSearchParams({ session_key: sessionKey });
  return apiGet(`/api/session-summary?${q}`);
}

export async function saveSummaryRevision(
  sessionKey: string,
  title: string,
  body: string,
  editorLabel = "local-user"
): Promise<SessionSummaryState> {
  const q = new URLSearchParams({ session_key: sessionKey });
  return apiPost(`/api/session-summary?${q}`, { title, body, editorLabel });
}

export async function listSummaryRevisions(sessionKey: string): Promise<SummaryRevision[]> {
  const q = new URLSearchParams({ session_key: sessionKey });
  return apiGet(`/api/summary-revisions?${q}`);
}

export async function runPipelineStage(
  stage: "phase2" | "phase3" | "phase4",
  inputRef?: string
): Promise<PipelineRunRecord> {
  return apiPost("/api/pipeline/run", { stage, inputRef });
}

export async function listPipelineRuns(): Promise<PipelineRunRecord[]> {
  return apiGet("/api/pipeline/runs");
}

export async function fetchRuntimeSummary(): Promise<{ summary: string }> {
  return apiPost("/api/fetch-summary", {});
}
