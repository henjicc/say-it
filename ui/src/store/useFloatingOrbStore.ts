import { create } from "zustand";
import { CMD, cmd, type FloatingOrbSettings } from "@/lib/tauri";
import { normalizeFloatingOrbAppearance, type FloatingOrbAppearance } from "@/floating-orb/interaction";

interface FloatingOrbState {
  settings: FloatingOrbSettings;
  busy: boolean;
  error: string;
  hydrate: (settings: FloatingOrbSettings) => void;
  setEnabled: (enabled: boolean) => Promise<void>;
  setAutoEnter: (enabled: boolean) => Promise<void>;
  updateAppearance: (patch: Partial<FloatingOrbAppearance>) => Promise<void>;
}

let appearanceRevision = 0;
let settingsRevision = 0;

/** 开关类设置：后端返回完整设置，期间有新的快照到达时以快照为准。 */
async function toggle(command: string, enabled: boolean) {
  const { setState } = useFloatingOrbStore;
  const revision = settingsRevision;
  setState({ busy: true, error: "" });
  try {
    const settings = await cmd<FloatingOrbSettings>(command, { enabled });
    if (revision === settingsRevision) setState({ settings });
  } catch (error) {
    setState({ error: String(error) });
    throw error;
  } finally {
    setState({ busy: false });
  }
}

export const useFloatingOrbStore = create<FloatingOrbState>((set, get) => ({
  settings: {
    enabled: false,
    position: null,
    sizePercent: 30,
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
  setEnabled: (enabled) => toggle(CMD.setFloatingOrbEnabled, enabled),
  setAutoEnter: (enabled) => toggle(CMD.setFloatingOrbAutoEnter, enabled),
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
