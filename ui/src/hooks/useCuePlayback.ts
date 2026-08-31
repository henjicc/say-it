import { playCueKind } from "@/lib/cues";
import { useTauriEvent } from "./useTauriEvent";

interface CuePayload {
  which?: "start" | "end";
  kind?: string;
}

/** Any 监听器也会收到 emit_to；声音的发送和接收两端都必须限定窗口。 */
export function useCuePlayback(event: string, target: string) {
  useTauriEvent<CuePayload>(event, (payload) => {
    if ((payload.which === "start" || payload.which === "end") && payload.kind) {
      playCueKind(payload.kind, payload.which);
    }
  }, true, target);
}
