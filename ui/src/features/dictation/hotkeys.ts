import { CMD, cmd } from "@/lib/tauri";
import { useDictationStore } from "@/store/useDictationStore";

type Tone = "" | "ok" | "err";

interface HotkeyHooks {
  setStatus: (text: string, tone?: Tone) => void;
  getRecording: () => boolean;
  isAssistantActive: () => boolean;
  toggleDictation: () => void | Promise<void>;
  onCancelKey: () => void | Promise<void>;
}

let hooks: HotkeyHooks = {
  setStatus: () => {},
  getRecording: () => false,
  isAssistantActive: () => false,
  toggleDictation: () => {},
  onCancelKey: () => {},
};

// ---- 快捷键 ----
let dictKeyCode = "CapsLock";
let dictCtrl = false;
let dictShift = false;
let dictAlt = false;
let dictMeta = false;
let dictInjectMethod: "paste" | "type" = "paste";
let dictCapturing = false;

export function configureHotkeys(next: HotkeyHooks) {
  hooks = next;
}

// ---- 快捷键显示 ----
const KEY_DISPLAY_NAMES: Record<string, string> = {
  CapsLock: "Caps Lock",
  Space: "Space",
  Enter: "Enter",
  Tab: "Tab",
  Backquote: "`",
  ArrowLeft: "←",
  ArrowUp: "↑",
  ArrowRight: "→",
  ArrowDown: "↓",
  PageUp: "PgUp",
  PageDown: "PgDn",
  ScrollLock: "Scroll Lock",
  NumLock: "Num Lock",
  PrintScreen: "PrtSc",
};

function prettyKeyName(code: string): string {
  if (!code) return "";
  if (KEY_DISPLAY_NAMES[code]) return KEY_DISPLAY_NAMES[code];
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (/^F\d{1,2}$/.test(code)) return code;
  return code;
}

export function comboLabel(): string {
  const parts: string[] = [];
  if (dictCtrl) parts.push("Ctrl");
  if (dictAlt) parts.push("Alt");
  if (dictShift) parts.push("Shift");
  if (dictMeta) parts.push("Win");
  parts.push(prettyKeyName(dictKeyCode));
  return parts.join(" + ");
}

function updateShortcutDisplay() {
  useDictationStore.setState({ shortcutLabel: dictKeyCode ? comboLabel() : "" });
}

async function saveDictationSettings() {
  await cmd(CMD.setDictationSettings, {
    settings: {
      key_code: dictKeyCode,
      ctrl: dictCtrl,
      shift: dictShift,
      alt: dictAlt,
      meta: dictMeta,
      inject_method: dictInjectMethod,
    },
  });
}

// ---- 快捷键捕获 ----
function stopShortcutCapture() {
  dictCapturing = false;
  useDictationStore.setState({ capturing: false });
  window.removeEventListener("keydown", onCaptureKeydown, true);
}

async function onCaptureKeydown(event: KeyboardEvent) {
  event.preventDefault();
  event.stopPropagation();
  if (
    ["ControlLeft", "ControlRight", "ShiftLeft", "ShiftRight", "AltLeft", "AltRight", "MetaLeft", "MetaRight"].includes(
      event.code,
    )
  ) {
    return;
  }
  if (event.code === "Escape") {
    stopShortcutCapture();
    hooks.setStatus("已取消设置快捷键。");
    return;
  }

  const prev = { dictKeyCode, dictCtrl, dictShift, dictAlt, dictMeta };
  dictKeyCode = event.code;
  dictCtrl = event.ctrlKey;
  dictShift = event.shiftKey;
  dictAlt = event.altKey;
  dictMeta = event.metaKey;
  stopShortcutCapture();
  updateShortcutDisplay();
  try {
    await saveDictationSettings();
    hooks.setStatus(`快捷键已设为：${comboLabel()}（在任意软件中按下即可）`, "ok");
  } catch (error) {
    ({ dictKeyCode, dictCtrl, dictShift, dictAlt, dictMeta } = prev);
    updateShortcutDisplay();
    hooks.setStatus(`设置失败：${String(error)}`, "err");
  }
}

export function startShortcutCapture() {
  if (dictCapturing) {
    stopShortcutCapture();
    hooks.setStatus("已取消设置快捷键。");
    return;
  }
  dictCapturing = true;
  useDictationStore.setState({ capturing: true });
  window.addEventListener("keydown", onCaptureKeydown, true);
}

export function isCapturing() {
  return dictCapturing;
}

export function getInjectMethod() {
  return dictInjectMethod;
}

export async function setInjectMethod(method: "paste" | "type") {
  dictInjectMethod = method || "paste";
  useDictationStore.setState({ injectMethod: dictInjectMethod });
  try {
    await saveDictationSettings();
    hooks.setStatus(
      `注入方式已切换为：${dictInjectMethod === "type" ? "模拟逐字输入" : "剪贴板粘贴"}`,
      "ok",
    );
  } catch (error) {
    hooks.setStatus(`保存失败：${String(error)}`, "err");
  }
}

// ---- 焦点在本应用窗口时的热键兜底 ----
let focusHotkeyDown = false;

export interface KeySig {
  code?: string;
  ctrlKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
  metaKey?: boolean;
}

function matchesDictHotkey(e: KeySig): boolean {
  return (
    e.code === dictKeyCode &&
    !!e.ctrlKey === dictCtrl &&
    !!e.shiftKey === dictShift &&
    !!e.altKey === dictAlt &&
    !!e.metaKey === dictMeta
  );
}

function handleHotkeyKeydown(e: KeySig, preventDefault?: () => void) {
  if (dictCapturing) return;
  if (e.code === "Escape") {
    if (hooks.getRecording() || hooks.isAssistantActive()) {
      preventDefault?.();
      hooks.onCancelKey();
    }
    return;
  }
  if (!dictKeyCode || !matchesDictHotkey(e)) return;
  preventDefault?.();
  if (focusHotkeyDown) return;
  focusHotkeyDown = true;
  hooks.toggleDictation();
}

function handleHotkeyKeyup(code?: string) {
  if (code === dictKeyCode) focusHotkeyDown = false;
}

function onFocusHotkeyKeydown(e: KeyboardEvent) {
  handleHotkeyKeydown(e, () => e.preventDefault());
}

function onFocusHotkeyKeyup(e: KeyboardEvent) {
  handleHotkeyKeyup(e.code);
}

export function handleForwardedKeydown(e: KeySig) {
  handleHotkeyKeydown(e);
}

export function handleForwardedKeyup(code?: string) {
  handleHotkeyKeyup(code);
}

export function installFocusHotkeyFallback(): () => void {
  window.addEventListener("keydown", onFocusHotkeyKeydown, true);
  window.addEventListener("keyup", onFocusHotkeyKeyup, true);
  return () => {
    window.removeEventListener("keydown", onFocusHotkeyKeydown, true);
    window.removeEventListener("keyup", onFocusHotkeyKeyup, true);
  };
}

export async function loadDictationSettings() {
  try {
    const d = await cmd<{
      key_code?: string;
      ctrl?: boolean;
      shift?: boolean;
      alt?: boolean;
      meta?: boolean;
      inject_method?: "paste" | "type";
    }>(CMD.getDictationSettings);
    dictKeyCode = d.key_code || "CapsLock";
    dictCtrl = !!d.ctrl;
    dictShift = !!d.shift;
    dictAlt = !!d.alt;
    dictMeta = !!d.meta;
    dictInjectMethod = d.inject_method || "paste";
    useDictationStore.setState({ injectMethod: dictInjectMethod });
    updateShortcutDisplay();
    hooks.setStatus(`速记就绪，快捷键：${comboLabel()}`);
  } catch (error) {
    hooks.setStatus(`读取速记设置失败：${String(error)}`, "err");
  }
}
