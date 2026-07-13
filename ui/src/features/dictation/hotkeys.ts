import { CMD, cmd, cmdSilent } from "@/lib/tauri";
import { useDictationStore } from "@/store/useDictationStore";

type Tone = "" | "ok" | "err";

interface HotkeyHooks {
  setStatus: (text: string, tone?: Tone) => void;
}

let hooks: HotkeyHooks = {
  setStatus: () => {},
};

// ---- 快捷键 ----
let dictKeyCode = "CapsLock";
let dictCtrl = false;
let dictShift = false;
let dictAlt = false;
let dictMeta = false;
let dictInjectMethod: "paste" | "type" = "paste";
let dictPressHoldMode = false;
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
      press_hold_mode: dictPressHoldMode,
    },
  });
}

// ---- 快捷键捕获 ----
// 捕获期间通知 Rust 侧：任意锁定键（CapsLock/NumLock/ScrollLock）一律吞掉、只上报，
// 不会真的切换大小写等状态——否则拿 CapsLock 绑快捷键时，按下的瞬间会先切换一次
// 大小写，绑定后该键又被永久吞掉，导致没法再用它把状态切回去。
function stopShortcutCapture() {
  dictCapturing = false;
  useDictationStore.setState({ capturing: false });
  window.removeEventListener("keydown", onCaptureKeydown, true);
  cmdSilent(CMD.setHotkeyCapturing, { active: false });
}

async function completeCapture(
  code: string,
  mods: { ctrl: boolean; shift: boolean; alt: boolean; meta: boolean },
) {
  const prev = { dictKeyCode, dictCtrl, dictShift, dictAlt, dictMeta };
  dictKeyCode = code;
  dictCtrl = mods.ctrl;
  dictShift = mods.shift;
  dictAlt = mods.alt;
  dictMeta = mods.meta;
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

  await completeCapture(event.code, {
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

export function handleCaptureLockKey(vk: number) {
  if (!dictCapturing) return;
  const code = LOCK_KEY_VK_TO_CODE[vk];
  if (!code) return;
  completeCapture(code, { ctrl: false, shift: false, alt: false, meta: false });
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
  cmdSilent(CMD.setHotkeyCapturing, { active: true });
}

export async function clearShortcut() {
  const prev = { dictKeyCode, dictCtrl, dictShift, dictAlt, dictMeta };
  dictKeyCode = "";
  dictCtrl = false;
  dictShift = false;
  dictAlt = false;
  dictMeta = false;
  updateShortcutDisplay();
  try {
    await saveDictationSettings();
    hooks.setStatus("已清除语音输入快捷键，语音输入将无法通过全局快捷键触发。");
  } catch (error) {
    ({ dictKeyCode, dictCtrl, dictShift, dictAlt, dictMeta } = prev);
    updateShortcutDisplay();
    hooks.setStatus(`清除失败：${String(error)}`, "err");
  }
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

export async function setPressHoldMode(enabled: boolean) {
  const prev = dictPressHoldMode;
  dictPressHoldMode = enabled;
  useDictationStore.setState({ pressHoldMode: dictPressHoldMode });
  try {
    await saveDictationSettings();
    hooks.setStatus(enabled ? "已启用长按输入：按住快捷键开始，松开结束。" : "已切换为按一次开始、再按一次结束。", "ok");
  } catch (error) {
    dictPressHoldMode = prev;
    useDictationStore.setState({ pressHoldMode: dictPressHoldMode });
    hooks.setStatus(`保存失败：${String(error)}`, "err");
  }
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
      press_hold_mode?: boolean;
    }>(CMD.getDictationSettings);
    dictKeyCode = d.key_code ?? "";
    dictCtrl = !!d.ctrl;
    dictShift = !!d.shift;
    dictAlt = !!d.alt;
    dictMeta = !!d.meta;
    dictInjectMethod = d.inject_method || "paste";
    dictPressHoldMode = !!d.press_hold_mode;
    useDictationStore.setState({ injectMethod: dictInjectMethod, pressHoldMode: dictPressHoldMode });
    updateShortcutDisplay();
    hooks.setStatus(dictKeyCode ? `速记就绪，快捷键：${comboLabel()}` : "速记就绪，当前未设置全局快捷键。");
  } catch (error) {
    hooks.setStatus(`读取速记设置失败：${String(error)}`, "err");
  }
}
