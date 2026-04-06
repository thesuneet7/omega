export type SessionListItem = {
  session_key: string;
  file_path: string;
  started_at_epoch_secs: number;
  ended_at_epoch_secs: number;
  duration_secs: number;
  accepted_captures: number;
  total_events_seen: number;
  /** Present when the API returns it (from the app database). */
  summary_title?: string | null;
};

export type SummaryRevision = {
  id: number;
  summary_id: number;
  title: string;
  body: string;
  edited_at_epoch_secs: number;
  editor_label: string;
};

export type SessionBucket = {
  bucket_id: number;
  title: string;
  body: string;
};

export type SessionSummaryState = {
  session_key: string;
  title: string;
  body: string;
  source_bucket_ids: number[];
  revisions: SummaryRevision[];
  buckets?: SessionBucket[];
};

const BUCKET_STORE_VERSION = 1;

export function encodeBucketStorage(buckets: SessionBucket[]): string {
  return JSON.stringify({ version: BUCKET_STORE_VERSION, buckets });
}

export type PipelineRunRecord = {
  id: number;
  stage: string;
  input_ref: string;
  status: string;
  started_at_epoch_secs: number;
  ended_at_epoch_secs?: number;
  error_text?: string;
};

export type ApiUsageSnapshot = {
  estimated_cost_usd_total: number;
  monthly_limit_usd: number;
  usage_percent_of_limit: number;
  embedded_chars_total: number;
  phase4_input_chars_total: number;
  phase4_output_chars_total: number;
  estimated_embed_tokens: number;
  estimated_phase4_input_tokens: number;
  estimated_phase4_output_tokens: number;
  pricing_note: string;
};

export type SessionUsage = {
  session_key: string;
  estimated_cost_usd: number;
  embedded_chars: number;
  phase4_input_chars: number;
  phase4_output_chars: number;
};

export type ApiUsageResponse = {
  overall: ApiUsageSnapshot;
  session: SessionUsage | null;
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

export async function startSession(): Promise<{ status: string }> {
  return apiPost("/api/session/start", {});
}

export async function endSession(): Promise<{ summary: string }> {
  return apiPost("/api/session/end", {});
}

export async function getApiUsage(sessionKey?: string): Promise<ApiUsageResponse> {
  const q = sessionKey ? new URLSearchParams({ session_key: sessionKey }) : "";
  const suffix = q ? `?${q}` : "";
  return apiGet(`/api/usage${suffix}`);
}
