import { useEffect, useRef } from "react";

import { notifyRunEvent, onRunNotificationCandidate } from "./notification-settings-api";
import type { AgentRunEventV4 } from "./types";

const observedHashes = new Set<string>();
const observedOrder: string[] = [];
const MAX_OBSERVED_HASHES = 2_048;

function isNotificationCandidate(event: AgentRunEventV4): boolean {
  return ["run_completed", "run_failed", "run_needs_attention", "tool_approval_requested"]
    .includes(event.event.kind);
}

function claimObservedHash(eventHash: string): boolean {
  if (observedHashes.has(eventHash)) return false;
  observedHashes.add(eventHash);
  observedOrder.push(eventHash);
  if (observedOrder.length > MAX_OBSERVED_HASHES) {
    const oldest = observedOrder.shift();
    if (oldest) observedHashes.delete(oldest);
  }
  return true;
}

export function useRunNotifications(onFailure: (message: string) => void) {
  const failureRef = useRef(onFailure);
  failureRef.current = onFailure;

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onRunNotificationCandidate((event) => {
      if (disposed || !isNotificationCandidate(event) || !claimObservedHash(event.event_hash)) return;
      void notifyRunEvent(event.run_id, event.event_hash).then((result) => {
        if (!disposed && result.outcome === "failed") failureRef.current("系统通知发送失败");
      }).catch((reason) => {
        if (!disposed) failureRef.current(reason instanceof Error ? reason.message : String(reason));
      });
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    }).catch((reason) => {
      if (!disposed) failureRef.current(reason instanceof Error ? reason.message : String(reason));
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
}
