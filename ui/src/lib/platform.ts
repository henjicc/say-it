const userAgent = navigator.userAgent;

export const isMacOS = userAgent.includes("Macintosh");
export const isWindows = userAgent.includes("Windows");
export const systemOcrLabel = isMacOS ? "macOS 系统 OCR" : "Windows 系统 OCR";
export const contextDebugShortcutLabel = isMacOS ? "⌃ + ⇧ + F8" : "Ctrl + Shift + F8";
export const contextDebugShortcutHint = isMacOS ? "（部分键盘还需按 Fn）" : "";

export const shortcutModifierLabels = isMacOS
  ? { ctrl: "⌃", alt: "⌥", shift: "⇧", meta: "⌘" }
  : { ctrl: "Ctrl", alt: "Alt", shift: "Shift", meta: "Win" };
