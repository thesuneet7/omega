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
  /** Bump after pipeline-affecting actions to refresh immediately. */
  refreshToken?: number;
};

export function ApiUsageMeter({ sessionKey, refreshToken = 0 }: Props) {
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
          <strong>Selected session:</strong>{" "}
          {session ? formatUsd(session.estimated_cost_usd) : "— (no recorded pipeline cost yet)"}
        </p>
      ) : (
        <p className="usage-meter__session muted">Select a session to see its cost estimate.</p>
      )}
      <details className="usage-meter__details">
        <summary>Token estimates</summary>
        <ul className="usage-meter__stats">
          <li>Embeddings (in): ~{overall.estimated_embed_tokens.toLocaleString()} tok</li>
          <li>Phase 4 LLM (in): ~{overall.estimated_phase4_input_tokens.toLocaleString()} tok</li>
          <li>Phase 4 LLM (out): ~{overall.estimated_phase4_output_tokens.toLocaleString()} tok</li>
        </ul>
        <p className="usage-meter__note">{overall.pricing_note}</p>
      </details>
    </div>
  );
}
