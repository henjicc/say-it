import { EVT, type FloatingOrbSettings } from "@/lib/tauri";
import { useFloatingOrbStore } from "@/store/useFloatingOrbStore";
import { useTauriEvent } from "./useTauriEvent";

/** 主窗口投影来自菜单等其他窗口的后端设置变更，不反向保存。 */
export function useFloatingOrbSync() {
  useTauriEvent<FloatingOrbSettings>(EVT.floatingOrbConfig, (settings) => {
    useFloatingOrbStore.getState().hydrate(settings);
  });
}
