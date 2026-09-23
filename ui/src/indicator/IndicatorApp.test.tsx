import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const eventHandlers = vi.hoisted(() => new Map<string, (payload: unknown) => void>());
const readyEvents = vi.hoisted(() => new Set<string>());

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ startDragging: vi.fn() }),
}));
vi.mock("@/hooks/useTauriEvent", () => ({
  useTauriEvent: (event: string, handler: (payload: unknown) => void) => {
    eventHandlers.set(event, handler);
    return readyEvents.has(event);
  },
}));
vi.mock("@/hooks/useCuePlayback", () => ({ useCuePlayback: vi.fn() }));
vi.mock("@/lib/tauri", async (importOriginal) => {
  const original = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...original,
    cmd: vi.fn(),
    cmdSilent: vi.fn(),
    emitEvent: vi.fn(),
  };
});

import { CMD, EVT, cmdSilent } from "@/lib/tauri";
import { IndicatorApp } from "./IndicatorApp";

describe("dictation indicator presentation", () => {
  beforeEach(() => {
    eventHandlers.clear();
    readyEvents.clear();
    vi.mocked(cmdSilent).mockClear();
    vi.stubGlobal("ResizeObserver", class {
      observe() {}
      disconnect() {}
    });
    vi.stubGlobal("matchMedia", () => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }));
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("rehydrates a new subtitle window only after all presentation listeners are ready", () => {
    const view = render(<IndicatorApp />);
    readyEvents.add(EVT.indicatorState);
    readyEvents.add(EVT.indicatorText);
    readyEvents.add(EVT.indicatorConfig);
    view.rerender(<IndicatorApp />);
    expect(cmdSilent).not.toHaveBeenCalled();
    readyEvents.add(EVT.indicatorTranslation);
    view.rerender(<IndicatorApp />);
    expect(cmdSilent).toHaveBeenCalledWith(CMD.syncSubtitlePresentation, { rehydrate: true });
    expect(cmdSilent).toHaveBeenCalledTimes(1);
    act(() => eventHandlers.get(EVT.indicatorConfig)?.({ mode: "subtitle", subtitle: { displayMode: "replace" } }));
    act(() => eventHandlers.get(EVT.indicatorState)?.({ state: "subtitle" }));
    expect(view.container.querySelector("#wrap")).toHaveClass("subtitle-mode", "subtitle-replace");
    expect(cmdSilent).toHaveBeenCalledTimes(1);
  });

  it("shows only the waveform while recording and text during processing", () => {
    const view = render(<IndicatorApp />);

    act(() => eventHandlers.get(EVT.indicatorState)?.({ state: "recording" }));
    act(() => eventHandlers.get(EVT.indicatorWaveform)?.({
      active: true,
      level: 0.2,
      peaks: [0.1, 0.2, 0.3, 0.2, 0.1],
    }));

    const pill = view.container.querySelector("#pill");
    expect(pill).toHaveClass("recording", "pill-wave");
    expect(pill?.querySelector(".dot")).not.toBeInTheDocument();
    expect(screen.queryByText("聆听中…")).not.toBeInTheDocument();
    expect(pill?.querySelector(".orb-waveform")).toBeInTheDocument();
    expect(pill?.querySelectorAll(".orb-wave-bar")).toHaveLength(9);

    act(() => eventHandlers.get(EVT.indicatorState)?.({ state: "processing" }));
    expect(view.container.querySelector("#pill")).toHaveClass("processing");
    expect(view.container.querySelector("#pill .dot")).toBeInTheDocument();
    expect(screen.getByText("识别中…")).toBeInTheDocument();
    expect(view.container.querySelector("#pill .orb-waveform")).not.toBeInTheDocument();

    act(() => eventHandlers.get(EVT.indicatorState)?.({ state: "smartProcessing" }));
    expect(view.container.querySelector("#pill")).toHaveClass("processing");
    expect(screen.getByText("处理中…")).toBeInTheDocument();
    expect(view.container.querySelector("#pill .orb-waveform")).not.toBeInTheDocument();
  });

  it("presents clipboard delivery as guidance instead of an error", () => {
    const view = render(<IndicatorApp />);

    act(() => eventHandlers.get(EVT.indicatorState)?.({ state: "fallback" }));

    expect(screen.getByRole("status")).toHaveTextContent(
      "已经把结果放到你的剪贴板里了，你粘贴就可以用了",
    );
    expect(view.container.querySelector("#error-panel")).not.toBeInTheDocument();
    expect(view.container.querySelector("#pill")).not.toBeInTheDocument();
  });
});
