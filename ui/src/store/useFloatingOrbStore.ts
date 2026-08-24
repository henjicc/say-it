import { create } from "zustand";
import { CMD, cmd, type FloatingOrbSettings } from "@/lib/tauri";

interface FloatingOrbState {
  settings: FloatingOrbSettings;
  busy: boolean;
  error: string;
  hydrate: (settings: FloatingOrbSettings) => void;
  setEnabled: (enabled: boolean) => Promise<void>;
}

export const useFloatingOrbStore = create<FloatingOrbState>((set) => ({
  settings: { enabled: false, position: null, size: 56, opacity: 100, glassEnabled: false },
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
}));
