import { useEffect, useRef, useState } from "react";
import { getPhase1LiveStatus, type Phase1LiveStatus } from "../lib/api";

type Props = {
  /** Poll only while a session is actively capturing */
  active: boolean;
  /** Latest live status for parent (e.g. pause indicator in header). */
  onLiveStatus?: (live: Phase1LiveStatus | null) => void;
};

export function CaptureLiveStatusBanner({ active, onLiveStatus }: Props) {
  const [live, setLive] = useState<Phase1LiveStatus | null>(null);
  const onLiveStatusRef = useRef(onLiveStatus);
  onLiveStatusRef.current = onLiveStatus;

  useEffect(() => {
    if (!active) {
      setLive(null);
      onLiveStatusRef.current?.(null);
      return;
    }
    let cancelled = false;
    const tick = () => {
      void getPhase1LiveStatus()
        .then((data) => {
          if (!cancelled) {
            setLive(data);
            onLiveStatusRef.current?.(data);
          }
        })
        .catch(() => {
          if (!cancelled) {
            setLive(null);
            onLiveStatusRef.current?.(null);
          }
        });
    };
    tick();
    const id = window.setInterval(tick, 1200);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [active]);

  const names = (live?.blockedAppNames ?? []).map((s) => s.trim()).filter(Boolean);
  const paused = live?.capturePaused === true;

  if (!active) {
    return null;
  }

  return (
    <>
      {paused ? (
        <div className="capture-pause-banner" role="status" aria-live="polite">
          Capture is paused — no screenshots are taken until you resume.
        </div>
      ) : null}
      {names.length > 0 ? (
        <ul className="capture-live-list" aria-label="Apps blocked from capture this session">
          {names.map((name) => (
            <li key={name}>{name}</li>
          ))}
        </ul>
      ) : null}
    </>
  );
}
