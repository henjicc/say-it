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
let settingsRevision = 0;

export const useFloatingOrbStore = create<FloatingOrbState>((set, get) => ({
  settings: {
    enabled: false,
    position: null,
    sizePercent: 45,
    opacity: 100,
    glassEnabled: false,
    glassMaterial: "sidebar",
    glassTint: 8,
    glassBorder: 0,
    autoEnter: false,
  },
  busy: false,
  error: "",
  hydrate: (settings) => {
    settingsRevision += 1;
    set({ settings, error: "" });
  },
  setEnabled: async (enabled) => {
    const revision = settingsRevision;
    set({ busy: true, error: "" });
    try {
      const settings = await cmd<FloatingOrbSettings>(CMD.setFloatingOrbEnabled, { enabled });
      if (revision === settingsRevision) set({ settings });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    } finally {
      set({ busy: false });
    }
  },
  updateAppearance: async (patch) => {
    const revision = ++appearanceRevision;
    const snapshotRevision = settingsRevision;
    const current = get().settings;
    const appearance = normalizeFloatingOrbAppearance({ ...current, ...patch });
    set({ settings: { ...current, ...appearance }, error: "" });
    try {
      const settings = await cmd<FloatingOrbSettings>(CMD.setFloatingOrbAppearance, { ...appearance });
      if (revision === appearanceRevision && snapshotRevision === settingsRevision) {
        set({ settings, error: "" });
      }
    } catch (error) {
      if (revision === appearanceRevision) {
        set({ ...(snapshotRevision === settingsRevision ? { settings: current } : {}), error: String(error) });
      }
      throw error;
    }
  },
}));
