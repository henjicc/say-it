import { useEffect } from "react";
import { useUiStore, type LayoutMode } from "@/store/useUiStore";

/** 窗口宽度低于该值切到简单模式。 */
export const SIMPLE_BELOW = 720;
/** 窗口宽度达到该值才回到专业模式；与 SIMPLE_BELOW 之间的缓冲避免拖动边缘时来回切换。 */
export const PRO_FROM = 800;

export function nextLayoutMode(current: LayoutMode, width: number): LayoutMode {
  if (width < SIMPLE_BELOW) return "simple";
  if (width >= PRO_FROM) return "pro";
  return current;
}

/** 按主窗口宽度自动切换简单/专业模式；最大化自然落到专业模式。 */
export function useLayoutMode() {
  const setLayoutMode = useUiStore((state) => state.setLayoutMode);
  useEffect(() => {
    const sync = () => setLayoutMode(nextLayoutMode(useUiStore.getState().layoutMode, window.innerWidth));
    sync();
    window.addEventListener("resize", sync);
    return () => window.removeEventListener("resize", sync);
  }, [setLayoutMode]);
}
