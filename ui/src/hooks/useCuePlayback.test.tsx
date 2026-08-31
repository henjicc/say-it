import { StrictMode } from "react";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { listeners, playCueKind } = vi.hoisted(() => ({
  listeners: new Set<{ event: string; target?: string; handler: (event: { payload: unknown }) => void }>(),
  playCueKind: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(),
  listen: vi.fn(async (event: string, handler: (event: { payload: unknown }) => void, options?: { target: string }) => {
    const listener = { event, handler, target: options?.target };
    listeners.add(listener);
    return () => { listeners.delete(listener); };
  }),
}));
vi.mock("@/lib/cues", () => ({ playCueKind }));

import { EVT } from "@/lib/tauri";
import { useCuePlayback } from "./useCuePlayback";

function emitTo(target: string, event: string, payload: unknown) {
  // 与安装的 Tauri match_any_or_filter 一致：无目标的 Any 监听也会收到定向事件。
  for (const listener of listeners) {
    if (listener.event === event && (!listener.target || listener.target === target)) {
      listener.handler({ payload });
    }
  }
}

describe("window-scoped cue playback", () => {
  beforeEach(() => { listeners.clear(); playCueKind.mockReset(); });
  afterEach(cleanup);

  it("plays once with both orb and hidden indicator mounted in StrictMode", async () => {
    renderHook(() => {
      useCuePlayback(EVT.indicatorPlayCue, "floating-orb");
      useCuePlayback(EVT.indicatorPlayCue, "dictation-indicator");
      useCuePlayback(EVT.dictationPlayCue, "main");
    }, { wrapper: StrictMode });
    await waitFor(() => expect(listeners.size).toBe(3));
    expect([...listeners].map((listener) => listener.target)).toEqual([
      "floating-orb", "dictation-indicator", "main",
    ]);
    for (const [target, event] of [
      ["floating-orb", EVT.indicatorPlayCue],
      ["dictation-indicator", EVT.indicatorPlayCue],
      ["main", EVT.dictationPlayCue],
    ]) {
      playCueKind.mockClear();
      act(() => emitTo(target, event, { which: "start", kind: "beep-up" }));
      expect(playCueKind).toHaveBeenCalledExactlyOnceWith("beep-up", "start");
    }
  });

  it("does not play from a disposed subscription awaiting unlisten", async () => {
    const hook = renderHook(() => useCuePlayback(EVT.indicatorPlayCue, "floating-orb"));
    const stale = [...listeners][0];
    hook.unmount();
    act(() => stale.handler({ payload: { which: "end", kind: "beep-down" } }));
    expect(playCueKind).not.toHaveBeenCalled();
    await waitFor(() => expect(listeners.size).toBe(0));
  });
});
