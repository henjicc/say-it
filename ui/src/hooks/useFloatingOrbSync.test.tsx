import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FloatingOrbSettings } from "@/lib/tauri";

const { cmd, handlers } = vi.hoisted(() => ({
  cmd: vi.fn(),
  handlers: new Map<string, (settings: FloatingOrbSettings) => void>(),
}));
vi.mock("@/lib/tauri", () => ({
  CMD: { setFloatingOrbEnabled: "set_floating_orb_enabled", setFloatingOrbAppearance: "set_floating_orb_appearance" },
  EVT: { floatingOrbConfig: "floating-orb-config" },
  cmd,
  on: vi.fn(async (event: string, handler: (settings: FloatingOrbSettings) => void) => {
    handlers.set(event, handler);
    return () => { handlers.delete(event); };
  }),
}));
import { useFloatingOrbStore } from "@/store/useFloatingOrbStore";
import { useFloatingOrbSync } from "./useFloatingOrbSync";

const defaults = { ...useFloatingOrbStore.getState().settings, enabled: true };

describe("floating orb settings projection", () => {
  beforeEach(() => {
    cmd.mockReset();
    handlers.clear();
    useFloatingOrbStore.setState({ settings: defaults, busy: false, error: "" });
  });
  afterEach(cleanup);

  it("updates the main-window switch after menu closure without saving it again", async () => {
    const hook = renderHook(() => {
      useFloatingOrbSync();
      return useFloatingOrbStore((state) => state.settings);
    });
    await waitFor(() => expect(handlers.has("floating-orb-config")).toBe(true));
    act(() => handlers.get("floating-orb-config")?.({ ...defaults, enabled: false, autoEnter: true }));
    expect(hook.result.current).toMatchObject({ enabled: false, autoEnter: true });
    expect(cmd).not.toHaveBeenCalled();
    act(() => handlers.get("floating-orb-config")?.({ ...defaults, enabled: true }));
    expect(hook.result.current.enabled).toBe(true);
  });

  it("does not restore an old enabled value when an appearance response arrives late", async () => {
    let finish: (settings: FloatingOrbSettings) => void = () => {};
    cmd.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    const pending = useFloatingOrbStore.getState().updateAppearance({ opacity: 80 });
    const closed = { ...defaults, enabled: false, opacity: 80 };
    useFloatingOrbStore.getState().hydrate(closed);
    finish({ ...closed, enabled: true });
    await pending;
    expect(useFloatingOrbStore.getState().settings).toEqual(closed);
  });
});
