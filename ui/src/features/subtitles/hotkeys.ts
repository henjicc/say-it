import { CMD, cmd, cmdSilent } from "@/lib/tauri";
import { useSubtitleStore } from "@/store/useSubtitleStore";

type Tone = "" | "ok" | "err";

interface SubtitleHotkeyHooks {
  setStatus: (text: string, tone?: Tone) => void;
  toggle: () => void | Promise<void>;
}

let hooks: SubtitleHotkeyHooks = {
  setStatus: () => {},
  toggle: () => {},
};

// ---- 快捷键 ----
let subKeyCode = "";
let subCtrl = false;
let subShift = false;
let subAlt = false;
let subMeta = false;
let subCapturing = false;
const metaKeyLabel = navigator.userAgent.includes("Macintosh") ? "⌘" : "Win";

export function configureSubtitleHotkeys(next: SubtitleHotkeyHooks) {
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

export function subtitleComboLabel(): string {
  if (!subKeyCode) return "";
  const parts: string[] = [];
  if (subCtrl) parts.push("Ctrl");
  if (subAlt) parts.push("Alt");
  if (subShift) parts.push("Shift");
  if (subMeta) parts.push(metaKeyLabel);
  parts.push(prettyKeyName(subKeyCode));
  return parts.join(" + ");
}

function updateSubtitleShortcutDisplay() {
  useSubtitleStore.getState().setRuntime({ shortcutLabel: subtitleComboLabel() });
}

async function saveSubtitleShortcut() {
  await cmd(CMD.setSubtitleShortcut, {
    settings: {
      key_code: subKeyCode,
      ctrl: subCtrl,
      shift: subShift,
      alt: subAlt,
      meta: subMeta,
    },
  });
}

// ---- 快捷键捕获 ----
// 捕获期间通知 Rust 侧：任意锁定键（CapsLock/NumLock/ScrollLock）一律吞掉、只上报，
// 不会真的切换大小写等状态——否则拿 CapsLock 绑快捷键时，按下的瞬间会先切换一次
// 大小写，绑定后该键又被永久吞掉，导致没法再用它把状态切回去。
function stopSubtitleShortcutCapture() {
  subCapturing = false;
  useSubtitleStore.getState().setRuntime({ capturing: false });
  window.removeEventListener("keydown", onCaptureKeydown, true);
  cmdSilent(CMD.setHotkeyCapturing, { active: false });
}

async function completeSubtitleCapture(
  code: string,
  mods: { ctrl: boolean; shift: boolean; alt: boolean; meta: boolean },
) {
  const prev = { subKeyCode, subCtrl, subShift, subAlt, subMeta };
  subKeyCode = code;
  subCtrl = mods.ctrl;
  subShift = mods.shift;
  subAlt = mods.alt;
  subMeta = mods.meta;
  stopSubtitleShortcutCapture();
  updateSubtitleShortcutDisplay();
  try {
    await saveSubtitleShortcut();
    hooks.setStatus(`实时字幕快捷键已设为：${subtitleComboLabel()}（在任意软件中按下即可）`, "ok");
  } catch (error) {
    ({ subKeyCode, subCtrl, subShift, subAlt, subMeta } = prev);
    updateSubtitleShortcutDisplay();
    hooks.setStatus(`设置失败：${String(error)}`, "err");
  }
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
    stopSubtitleShortcutCapture();
    hooks.setStatus("已取消设置快捷键。");
    return;
  }

  await completeSubtitleCapture(event.code, {
    ctrl: event.ctrlKey,
    shift: event.shiftKey,
    alt: event.altKey,
    meta: event.metaKey,
  });
}

// 锁定键被 Rust 侧吞掉后不会产生 DOM keydown 事件，只能靠这个上报的虚拟键码识别；
// 这几个键的绑定几乎总是单独使用，因此不追加修饰键状态。
const LOCK_KEY_VK_TO_CODE: Record<number, string> = {
  0x14: "CapsLock",
  0x90: "NumLock",
  0x91: "ScrollLock",
};

export function handleSubtitleCaptureLockKey(vk: number) {
  if (!subCapturing) return;
  const code = LOCK_KEY_VK_TO_CODE[vk];
  if (!code) return;
  completeSubtitleCapture(code, { ctrl: false, shift: false, alt: false, meta: false });
}

export function startSubtitleShortcutCapture() {
  if (subCapturing) {
    stopSubtitleShortcutCapture();
    hooks.setStatus("已取消设置快捷键。");
    return;
  }
  subCapturing = true;
  useSubtitleStore.getState().setRuntime({ capturing: true });
  window.addEventListener("keydown", onCaptureKeydown, true);
  cmdSilent(CMD.setHotkeyCapturing, { active: true });
}

export async function clearSubtitleShortcut() {
  const prev = { subKeyCode, subCtrl, subShift, subAlt, subMeta };
  subKeyCode = "";
  subCtrl = false;
  subShift = false;
  subAlt = false;
  subMeta = false;
  updateSubtitleShortcutDisplay();
  try {
    await saveSubtitleShortcut();
    hooks.setStatus("已清除实时字幕快捷键。");
  } catch (error) {
    ({ subKeyCode, subCtrl, subShift, subAlt, subMeta } = prev);
    updateSubtitleShortcutDisplay();
    hooks.setStatus(`清除失败：${String(error)}`, "err");
  }
}

export function isSubtitleCapturing() {
  return subCapturing;
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

function matchesSubtitleHotkey(e: KeySig): boolean {
  return (
    !!subKeyCode &&
    e.code === subKeyCode &&
    !!e.ctrlKey === subCtrl &&
    !!e.shiftKey === subShift &&
    !!e.altKey === subAlt &&
    !!e.metaKey === subMeta
  );
}

function handleSubtitleHotkeyKeydown(e: KeySig, preventDefault?: () => void) {
  if (subCapturing) return;
  if (!matchesSubtitleHotkey(e)) return;
  preventDefault?.();
  if (focusHotkeyDown) return;
  focusHotkeyDown = true;
  hooks.toggle();
}

function handleSubtitleHotkeyKeyup(code?: string) {
  if (code === subKeyCode) focusHotkeyDown = false;
}

function onFocusHotkeyKeydown(e: KeyboardEvent) {
  handleSubtitleHotkeyKeydown(e, () => e.preventDefault());
}

function onFocusHotkeyKeyup(e: KeyboardEvent) {
  handleSubtitleHotkeyKeyup(e.code);
}

export function handleForwardedSubtitleKeydown(e: KeySig) {
  handleSubtitleHotkeyKeydown(e);
}

export function handleForwardedSubtitleKeyup(code?: string) {
  handleSubtitleHotkeyKeyup(code);
}

export function installSubtitleFocusHotkeyFallback(): () => void {
  window.addEventListener("keydown", onFocusHotkeyKeydown, true);
  window.addEventListener("keyup", onFocusHotkeyKeyup, true);
  return () => {
    window.removeEventListener("keydown", onFocusHotkeyKeydown, true);
    window.removeEventListener("keyup", onFocusHotkeyKeyup, true);
  };
}

export async function loadSubtitleShortcut() {
  try {
    const d = await cmd<{
      key_code?: string;
      ctrl?: boolean;
      shift?: boolean;
      alt?: boolean;
      meta?: boolean;
    }>(CMD.getSubtitleShortcut);
    subKeyCode = d.key_code || "";
    subCtrl = !!d.ctrl;
    subShift = !!d.shift;
    subAlt = !!d.alt;
    subMeta = !!d.meta;
    updateSubtitleShortcutDisplay();
  } catch (error) {
    hooks.setStatus(`读取实时字幕快捷键失败：${String(error)}`, "err");
  }
}
