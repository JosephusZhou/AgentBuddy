import { useCallback, useEffect, useRef, useState } from "react";

const DEFAULT_TIMEOUT_MS = 3_000;

/**
 * Status toast text under page titles.
 * Non-empty messages auto-clear after `timeoutMs` (default 3s).
 * Setting a new message resets the timer; setting "" clears immediately.
 */
export function useStatusMessage(timeoutMs = DEFAULT_TIMEOUT_MS) {
  const [statusMsg, setStatusMsgState] = useState("");
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const clearTimer = useCallback(() => {
    if (timerRef.current != null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const setStatusMsg = useCallback(
    (msg: string) => {
      clearTimer();
      setStatusMsgState(msg);
      if (msg) {
        timerRef.current = setTimeout(() => {
          setStatusMsgState("");
          timerRef.current = null;
        }, timeoutMs);
      }
    },
    [clearTimer, timeoutMs],
  );

  useEffect(() => () => clearTimer(), [clearTimer]);

  return [statusMsg, setStatusMsg] as const;
}
