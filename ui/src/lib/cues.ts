import { CMD, cmdSilent } from "@/lib/tauri";

/** 设置页试听与听写运行时共用 Rust 原生输出，不依赖 WebView AudioContext。 */
export function playCue(which: "start" | "end") {
  void cmdSilent(CMD.previewDictationCue, { which });
}
