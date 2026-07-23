const userAgent = navigator.userAgent;

export const isMacOS = userAgent.includes("Macintosh");
export const isWindows = userAgent.includes("Windows");

export const shortcutModifierLabels = isMacOS
  ? { ctrl: "⌃", alt: "⌥", shift: "⇧", meta: "⌘" }
  : { ctrl: "Ctrl", alt: "Alt", shift: "Shift", meta: "Win" };
