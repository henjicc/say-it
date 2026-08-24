import { beforeEach, describe, expect, it, vi } from "vitest";

const { cmd } = vi.hoisted(() => ({ cmd: vi.fn() }));

vi.mock("@/lib/tauri", () => ({
  CMD: { setMouseGestureSettings: "set_mouse_gesture_settings" },
  cmd,
}));

import { useMouseGestureStore } from "./useMouseGestureStore";

const defaults = {
  enabled: false,
  mode: "confirm" as const,
  sensitivity: 50,
  available: false,
  error: null,
};

describe("mouse gesture store", () => {
  beforeEach(() => {
    cmd.mockReset();
    useMouseGestureStore.setState({ settings: defaults, busy: false, error: "" });
  });

  it("hydrates the persisted snapshot", () => {
    useMouseGestureStore.getState().hydrate({
      enabled: true,
      mode: "direct",
      sensitivity: 75,
      available: true,
      error: null,
    });
    expect(useMouseGestureStore.getState().settings).toMatchObject({
      enabled: true,
      mode: "direct",
      sensitivity: 75,
    });
  });

  it("clamps sensitivity and atomically saves the full setting", async () => {
    cmd.mockResolvedValue({
      enabled: true,
      mode: "confirm",
      sensitivity: 100,
      available: true,
      error: null,
    });
    await useMouseGestureStore.getState().update({ enabled: true, sensitivity: 140 });
    expect(cmd).toHaveBeenCalledWith("set_mouse_gesture_settings", {
      enabled: true,
      mode: "confirm",
      sensitivity: 100,
    });
  });
});
