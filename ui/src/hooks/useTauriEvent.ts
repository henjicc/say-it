import { useEffect, useRef } from "react";
import { on } from "@/lib/tauri";

/**
 * 订阅一个 Tauri 事件，组件卸载时自动取消。
 * handler 用 ref 持有，避免因 handler 变化导致反复重订阅。
 */
export function useTauriEvent<T = unknown>(
  event: string,
  handler: (payload: T) => void,
  enabled = true,
) {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    if (!enabled) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    on<T>(event, (payload) => handlerRef.current(payload)).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [event, enabled]);
}
