import { useEffect, useRef, useState } from "react";
import { on } from "@/lib/tauri";

/**
 * 订阅一个 Tauri 事件，组件卸载时自动取消。
 * handler 用 ref 持有，避免因 handler 变化导致反复重订阅。
 * reportReady 为真时返回订阅完成状态，用于按需窗口的首次快照同步。
 */
export function useTauriEvent<T = unknown>(
  event: string,
  handler: (payload: T) => void,
  enabled = true,
  target?: string,
  reportReady = false,
) {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;
  const [ready, setReady] = useState(false);

  useEffect(() => {
    if (reportReady) setReady(false);
    if (!enabled) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    on<T>(event, (payload) => {
      if (!cancelled) handlerRef.current(payload);
    }, target).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
        if (reportReady) setReady(true);
      }
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [event, enabled, target, reportReady]);

  return enabled && ready;
}
