import { create } from "zustand";
import { CMD, cmd, type MouseGestureMode, type MouseGestureSnapshot } from "@/lib/tauri";

interface MouseGestureState {
  settings: MouseGestureSnapshot;
  busy: boolean;
  error: string;
  hydrate: (settings: MouseGestureSnapshot) => void;
  update: (patch: Partial<Pick<MouseGestureSnapshot, "enabled" | "mode" | "sensitivity" | "rapidClickEnabled" | "rapidClickCount">>) => Promise<void>;
}

let updateRevision = 0;

export const useMouseGestureStore = create<MouseGestureState>((set, get) => ({
  settings: {
    enabled: false,
    mode: "confirm" as MouseGestureMode,
    sensitivity: 50,
    rapidClickEnabled: true,
    rapidClickCount: 3,
    available: false,
    error: null,
  },
  busy: false,
  error: "",
  hydrate: (settings) => set({ settings, error: settings.error || "" }),
  update: async (patch) => {
    const revision = ++updateRevision;
    const previous = get().settings;
    const next = {
      enabled: patch.enabled ?? previous.enabled,
      mode: patch.mode ?? previous.mode,
      sensitivity: Math.max(0, Math.min(100, Math.round(patch.sensitivity ?? previous.sensitivity))),
      rapidClickEnabled: patch.rapidClickEnabled ?? previous.rapidClickEnabled,
      rapidClickCount: Math.max(3, Math.min(5, Math.round(patch.rapidClickCount ?? previous.rapidClickCount))),
    };
    set({ busy: true, error: "", settings: { ...previous, ...next } });
    try {
      const settings = await cmd<MouseGestureSnapshot>(CMD.setMouseGestureSettings, next);
      if (revision === updateRevision) set({ settings, error: settings.error || "" });
    } catch (error) {
      if (revision === updateRevision) set({ settings: previous, error: String(error) });
      throw error;
    } finally {
      if (revision === updateRevision) set({ busy: false });
    }
  },
}));
