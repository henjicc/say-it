import { create } from "zustand";

export interface Toast {
  id: number;
  text: string;
  tone: "info" | "err";
}

interface ToastState {
  toast: Toast | null;
  dismiss: (id: number) => void;
}

export const useToastStore = create<ToastState>((set) => ({
  toast: null,
  dismiss: (id) => set((state) => (state.toast?.id === id ? { toast: null } : {})),
}));

let nextId = 0;

/**
 * 操作结果的轻提示，同一时刻只显示最新一条。
 * 只用于结果在界面上看不出来的操作（复制、清空、后台完成）和不属于某一行的错误；
 * 结果本身可见的不提示；能归到某一行的错误优先写在该行下方。
 */
export function notify(text: string, tone: Toast["tone"] = "info") {
  useToastStore.setState({ toast: { id: ++nextId, text, tone } });
}

export function notifyError(error: unknown) {
  notify(String(error), "err");
}
