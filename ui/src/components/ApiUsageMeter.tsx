import { useCallback, useEffect, useState } from "react";
import { getApiUsage, type ApiUsageResponse } from "../lib/api";

function formatUsd(n: number): string {
  if (n < 0.01 && n > 0) {
    return n.toLocaleString(undefined, { style: "currency", currency: "USD", maximumFractionDigits: 4 });
  }
  return n.toLocaleString(undefined, { style: "currency", currency: "USD", minimumFractionDigits: 2, maximumFractionDigits: 2 });
}

type Props = {
  sessionKey: string | null;
  /** Human-readable session name for the selected row (not the internal key). */
  sessionLabel?: string | null;
  /** Bump after pipeline-affecting actions to refresh immediately. */
  refreshToken?: number;
};

export function ApiUsageMeter({ sessionKey, sessionLabel, refreshToken = 0 }: Props) {
  const [data, setData] = useState<ApiUsageResponse | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const u = await getApiUsage(sessionKey ?? undefined);
      setData(u);
      setErr(null);
    } catch (e) {
      setErr((e as Error).message);
    }
  }, [sessionKey]);

  useEffect(() => {
    void refresh();
  }, [refresh, refreshToken]);

  useEffect(() => {
    const id = setInterval(() => void refresh(), 8000);
    return () => clearInterval(id);
  }, [refresh]);

  if (err) {
    return (
      <div className="usage-meter usage-meter--error">
        <p className="usage-meter__title">API usage</p>
        <p className="usage-meter__err">{err}</p>
      </div>
    );
  }

  if (!data) {
    return (
      <div className="usage-meter">
        <p className="usage-meter__title">API usage</p>
        <p className="muted">Loading…</p>
      </div>
    );
  }

  const { overall, session } = data;
  const pct = Math.min(100, Math.max(0, overall.usage_percent_of_limit));
  const estimatedTokenTotal =
    overall.estimated_embed_tokens +
    overall.estimated_phase4_input_tokens +
    overall.estimated_phase4_output_tokens;

  return (
    <div className="usage-meter">
      <div className="usage-meter__header">
        <p className="usage-meter__title">Estimated API spend</p>
        <span className="usage-meter__badge">
          {formatUsd(overall.estimated_cost_usd_total)} / {formatUsd(overall.monthly_limit_usd)}
        </span>
      </div>
      <div className="usage-bar" aria-label="Usage versus monthly planning limit">
        <div className="usage-bar__fill" style={{ width: `${pct}%` }} />
      </div>
      <p className="usage-meter__sub">
        {pct.toFixed(0)}% of monthly planning cap ({formatUsd(overall.monthly_limit_usd)}). Set{" "}
        <code className="usage-meter__code">OMEGA_USAGE_MONTHLY_LIMIT_USD</code> to adjust.
      </p>
      {sessionKey ? (
        <p className="usage-meter__session">
          <strong>This session</strong>
          {sessionLabel ? (
            <>
              {" "}
              <span className="usage-meter__session-name">({sessionLabel})</span>
            </>
          ) : null}
          : {session ? formatUsd(session.estimated_cost_usd) : "— no API cost recorded yet"}
        </p>
      ) : (
        <p className="usage-meter__session muted">Choose a session to see its estimated cost.</p>
      )}
      <p className="usage-meter__token-total">
        ~{estimatedTokenTotal.toLocaleString()} tokens estimated (all sessions, this month).
      </p>
      <details className="usage-meter__details">
        <summary>Breakdown by step</summary>
        <ul className="usage-meter__stats">
          <li>Embeddings: ~{overall.estimated_embed_tokens.toLocaleString()} tokens</li>
          <li>Summary model (prompt): ~{overall.estimated_phase4_input_tokens.toLocaleString()} tokens</li>
          <li>Summary model (reply): ~{overall.estimated_phase4_output_tokens.toLocaleString()} tokens</li>
        </ul>
        <p className="usage-meter__note">{overall.pricing_note}</p>
      </details>
    </div>
  );
}
