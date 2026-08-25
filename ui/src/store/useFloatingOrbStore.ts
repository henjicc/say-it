import { create } from "zustand";
import { CMD, cmd, type FloatingOrbSettings } from "@/lib/tauri";
import { normalizeFloatingOrbAppearance, type FloatingOrbAppearance } from "@/floating-orb/interaction";

interface FloatingOrbState {
  settings: FloatingOrbSettings;
  busy: boolean;
  error: string;
  hydrate: (settings: FloatingOrbSettings) => void;
  setEnabled: (enabled: boolean) => Promise<void>;
  updateAppearance: (patch: Partial<FloatingOrbAppearance>) => Promise<void>;
}

let appearanceRevision = 0;

export const useFloatingOrbStore = create<FloatingOrbState>((set, get) => ({
  settings: {
    enabled: false,
    position: null,
    sizePercent: 45,
    opacity: 40,
    glassEnabled: false,
    glassMaterial: "sidebar",
    glassTint: 8,
    glassBorder: 0,
    autoEnter: false,
  },
  busy: false,
  error: "",
  hydrate: (settings) => set({ settings, error: "" }),
  setEnabled: async (enabled) => {
    set({ busy: true, error: "" });
    try {
      const settings = await cmd<FloatingOrbSettings>(CMD.setFloatingOrbEnabled, { enabled });
      set({ settings });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    } finally {
      set({ busy: false });
    }
  },
  updateAppearance: async (patch) => {
    const revision = ++appearanceRevision;
    const current = get().settings;
    const appearance = normalizeFloatingOrbAppearance({ ...current, ...patch });
    set({ settings: { ...current, ...appearance }, error: "" });
    try {
      const settings = await cmd<FloatingOrbSettings>(CMD.setFloatingOrbAppearance, { ...appearance });
      if (revision === appearanceRevision) set({ settings, error: "" });
    } catch (error) {
      if (revision === appearanceRevision) set({ settings: current, error: String(error) });
      throw error;
    }
  },
}));
