import { useCallback, useRef } from "react";
import { message } from "@tauri-apps/plugin-dialog";

export function useErrorDetailsDialog() {
  const open = useRef(false);
  return useCallback(async (details: string) => {
    if (open.current) return;
    open.current = true;
    try {
      // 使用独立原生弹窗，避免长错误被悬浮球的小窗口裁切；传值保留当次错误。
      await message(details || "语音输入失败，未提供具体错误信息。", {
        title: "语音输入出错",
        kind: "error",
        buttons: { ok: "关闭" },
      });
    } finally {
      open.current = false;
    }
  }, []);
}
