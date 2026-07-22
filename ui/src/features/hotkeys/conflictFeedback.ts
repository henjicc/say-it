import { create } from "zustand";

const SHORTCUT_CONFLICT_PREFIX = "快捷键冲突：";

interface ShortcutConflictState {
  message: string | null;
  show: (message: string) => void;
  close: () => void;
}

export const useShortcutConflictStore = create<ShortcutConflictState>((set) => ({
  message: null,
  show: (message) => set({ message }),
  close: () => set({ message: null }),
}));

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error || "未知错误");
}

/** 仅接管后端明确标记的快捷键冲突，其他保存错误仍由原页面展示。 */
export function reportShortcutConflict(error: unknown): boolean {
  const text = errorText(error);
  const marker = text.indexOf(SHORTCUT_CONFLICT_PREFIX);
  if (marker < 0) return false;
  const message = text.slice(marker + SHORTCUT_CONFLICT_PREFIX.length).trim();
  useShortcutConflictStore.getState().show(message || "当前快捷键已被其他功能使用，不能重复设置。");
  return true;
}
