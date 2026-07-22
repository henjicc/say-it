import { CMD, cmd, cmdSilent } from "@/lib/tauri";
import { useDictationStore } from "@/store/useDictationStore";
import { reportShortcutConflict } from "@/features/hotkeys/conflictFeedback";

type Tone = "" | "ok" | "err";

interface HotkeyHooks {
  setStatus: (text: string, tone?: Tone) => void;
}

export interface ShortcutCombo {
  keyCode: string;
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  meta: boolean;
}

export type ShortcutProcessingMode =
  | "followScene"
  | "raw"
  | "localOnly"
  | "smartOnly"
  | "smartAndLocal";

export type ShortcutTriggerMode = "toggle" | "pressHold";

export interface DictationShortcutProfile extends ShortcutCombo {
  id: string;
  name: string;
  enabled: boolean;
  triggerMode: ShortcutTriggerMode;
  processingMode: ShortcutProcessingMode;
  smartTemplateId: string | null;
  smartProcessingMinChars: number | null;
  injectMethod: "paste" | "type" | null;
}

export const MAX_DICTATION_SHORTCUT_PROFILES = 8;

let hooks: HotkeyHooks = { setStatus: () => {} };
let mainShortcut: ShortcutCombo = {
  keyCode: "CapsLock",
  ctrl: false,
  shift: false,
  alt: false,
  meta: false,
};
let dictInjectMethod: "paste" | "type" = "paste";
let dictPressHoldMode = false;
let dictShortcutProfiles: DictationShortcutProfile[] = [];
let settingsMutationRevision = 0;
const metaKeyLabel = navigator.userAgent.includes("Macintosh") ? "⌘" : "Win";

interface DictationSettingsSnapshot {
  mainShortcut: ShortcutCombo;
  injectMethod: "paste" | "type";
  pressHoldMode: boolean;
  shortcutProfiles: DictationShortcutProfile[];
}

let persistedSettings: DictationSettingsSnapshot | null = null;
let settingsSaveQueue: Promise<void> = Promise.resolve();

export function configureHotkeys(next: HotkeyHooks) {
  hooks = next;
}

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
  return code;
}

export function shortcutLabel(shortcut: ShortcutCombo): string {
  if (!shortcut.keyCode) return "";
  const parts: string[] = [];
  if (shortcut.ctrl) parts.push("Ctrl");
  if (shortcut.alt) parts.push("Alt");
  if (shortcut.shift) parts.push("Shift");
  if (shortcut.meta) parts.push(metaKeyLabel);
  parts.push(prettyKeyName(shortcut.keyCode));
  return parts.join(" + ");
}

export function shortcutSignature(shortcut: ShortcutCombo): string {
  return [
    shortcut.ctrl ? "1" : "0",
    shortcut.shift ? "1" : "0",
    shortcut.alt ? "1" : "0",
    shortcut.meta ? "1" : "0",
    shortcut.keyCode,
  ].join(":");
}

export function comboLabel(): string {
  return shortcutLabel(mainShortcut);
}

function publishSettingsState() {
  useDictationStore.setState({
    shortcutLabel: comboLabel(),
    shortcut: { ...mainShortcut },
    shortcutProfiles: dictShortcutProfiles.map((profile) => ({ ...profile })),
    injectMethod: dictInjectMethod,
    pressHoldMode: dictPressHoldMode,
  });
}

function settingsSnapshot(): DictationSettingsSnapshot {
  return {
    mainShortcut: { ...mainShortcut },
    injectMethod: dictInjectMethod,
    pressHoldMode: dictPressHoldMode,
    shortcutProfiles: dictShortcutProfiles.map((profile) => ({ ...profile })),
  };
}

function applySettingsSnapshot(snapshot: DictationSettingsSnapshot) {
  mainShortcut = { ...snapshot.mainShortcut };
  dictInjectMethod = snapshot.injectMethod;
  dictPressHoldMode = snapshot.pressHoldMode;
  dictShortcutProfiles = snapshot.shortcutProfiles.map((profile) => ({ ...profile }));
  publishSettingsState();
}

async function sendDictationSettings(snapshot: DictationSettingsSnapshot) {
  await cmd(CMD.setDictationSettings, {
    settings: {
      key_code: snapshot.mainShortcut.keyCode,
      ctrl: snapshot.mainShortcut.ctrl,
      shift: snapshot.mainShortcut.shift,
      alt: snapshot.mainShortcut.alt,
      meta: snapshot.mainShortcut.meta,
      inject_method: snapshot.injectMethod,
      press_hold_mode: snapshot.pressHoldMode,
      shortcut_profiles: snapshot.shortcutProfiles,
    },
  });
}

async function persistCurrentSettings() {
  const snapshot = settingsSnapshot();
  const revision = ++settingsMutationRevision;
  const pending = settingsSaveQueue.then(() => sendDictationSettings(snapshot));
  settingsSaveQueue = pending.catch(() => {});
  try {
    await pending;
    persistedSettings = snapshot;
  } catch (error) {
    if (revision === settingsMutationRevision && persistedSettings) {
      applySettingsSnapshot(persistedSettings);
    }
    throw error;
  }
}

interface ActiveCapture {
  complete: (shortcut: ShortcutCombo) => void | Promise<void>;
  cancel: () => void;
}

let activeCapture: ActiveCapture | null = null;

function teardownCapture(notifyCancel: boolean) {
  const capture = activeCapture;
  activeCapture = null;
  window.removeEventListener("keydown", onCaptureKeydown, true);
  cmdSilent(CMD.setHotkeyCapturing, { active: false });
  if (notifyCancel) capture?.cancel();
}

export function beginShortcutCapture(
  complete: (shortcut: ShortcutCombo) => void | Promise<void>,
  cancel: () => void,
): () => void {
  teardownCapture(true);
  const capture = { complete, cancel };
  activeCapture = capture;
  window.addEventListener("keydown", onCaptureKeydown, true);
  cmdSilent(CMD.setHotkeyCapturing, { active: true });
  return () => {
    if (activeCapture === capture) teardownCapture(true);
  };
}

async function finishCapture(shortcut: ShortcutCombo) {
  const capture = activeCapture;
  if (!capture) return;
  teardownCapture(false);
  await capture.complete(shortcut);
}

async function onCaptureKeydown(event: KeyboardEvent) {
  event.preventDefault();
  event.stopPropagation();
  if (
    ["ControlLeft", "ControlRight", "ShiftLeft", "ShiftRight", "AltLeft", "AltRight", "MetaLeft", "MetaRight"].includes(
      event.code,
    )
  ) return;
  if (event.code === "Escape") {
    teardownCapture(true);
    return;
  }
  await finishCapture({
    keyCode: event.code,
    ctrl: event.ctrlKey,
    shift: event.shiftKey,
    alt: event.altKey,
    meta: event.metaKey,
  });
}

const LOCK_KEY_VK_TO_CODE: Record<number, string> = {
  0x14: "CapsLock",
  0x90: "NumLock",
  0x91: "ScrollLock",
};

export function handleCaptureLockKey(vk: number) {
  const code = LOCK_KEY_VK_TO_CODE[vk];
  if (!code || !activeCapture) return;
  void finishCapture({ keyCode: code, ctrl: false, shift: false, alt: false, meta: false });
}

export async function setMainShortcut(shortcut: ShortcutCombo) {
  mainShortcut = { ...shortcut };
  publishSettingsState();
  try {
    await persistCurrentSettings();
    hooks.setStatus(`主快捷键已设为：${comboLabel()}`, "ok");
  } catch (error) {
    if (!reportShortcutConflict(error)) hooks.setStatus(`设置失败：${String(error)}`, "err");
    throw error;
  }
}

export function isCapturing() {
  return activeCapture !== null;
}

export function getInjectMethod() {
  return dictInjectMethod;
}

export async function setInjectMethod(method: "paste" | "type") {
  dictInjectMethod = method === "type" ? "type" : "paste";
  publishSettingsState();
  try {
    await persistCurrentSettings();
    hooks.setStatus(
      `注入方式已切换为：${dictInjectMethod === "type" ? "模拟逐字输入" : "剪贴板粘贴"}`,
      "ok",
    );
  } catch (error) {
    hooks.setStatus(`保存失败：${String(error)}`, "err");
  }
}

export async function setPressHoldMode(enabled: boolean) {
  dictPressHoldMode = enabled;
  publishSettingsState();
  try {
    await persistCurrentSettings();
    hooks.setStatus(enabled ? "已启用长按输入：按住快捷键开始，松开结束。" : "已切换为按一次开始、再按一次结束。", "ok");
  } catch (error) {
    if (!reportShortcutConflict(error)) hooks.setStatus(`保存失败：${String(error)}`, "err");
  }
}

export async function updateShortcutProfiles(next: DictationShortcutProfile[]) {
  dictShortcutProfiles = next.map((profile) => ({ ...profile }));
  publishSettingsState();
  try {
    await persistCurrentSettings();
    hooks.setStatus("快捷键方案已保存。", "ok");
  } catch (error) {
    if (!reportShortcutConflict(error)) hooks.setStatus(`快捷键方案保存失败：${String(error)}`, "err");
    throw error;
  }
}

export async function pruneShortcutProfileTemplates(validTemplateIds: Iterable<string>) {
  const valid = new Set(validTemplateIds);
  const next = dictShortcutProfiles.map((profile) =>
    profile.smartTemplateId && !valid.has(profile.smartTemplateId)
      ? { ...profile, smartTemplateId: null }
      : profile,
  );
  if (next.some((profile, index) => profile !== dictShortcutProfiles[index])) {
    await updateShortcutProfiles(next);
  }
}

function normalizeProfile(value: Partial<DictationShortcutProfile>): DictationShortcutProfile | null {
  if (typeof value.id !== "string") return null;
  const mode = value.processingMode;
  const processingMode: ShortcutProcessingMode =
    mode === "raw" || mode === "localOnly" || mode === "smartOnly" || mode === "smartAndLocal"
      ? mode
      : "followScene";
  return {
    id: value.id,
    name: typeof value.name === "string" ? value.name : "",
    enabled: value.enabled === true,
    triggerMode: value.triggerMode === "pressHold" ? "pressHold" : "toggle",
    keyCode: typeof value.keyCode === "string" ? value.keyCode : "",
    ctrl: value.ctrl === true,
    shift: value.shift === true,
    alt: value.alt === true,
    meta: value.meta === true,
    processingMode,
    smartTemplateId: typeof value.smartTemplateId === "string" && value.smartTemplateId ? value.smartTemplateId : null,
    smartProcessingMinChars:
      typeof value.smartProcessingMinChars === "number" && value.smartProcessingMinChars >= 0
        ? Math.min(10_000, Math.round(value.smartProcessingMinChars))
        : null,
    injectMethod: value.injectMethod === "paste" || value.injectMethod === "type" ? value.injectMethod : null,
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
      press_hold_mode?: boolean;
      shortcut_profiles?: Partial<DictationShortcutProfile>[];
    }>(CMD.getDictationSettings);
    mainShortcut = {
      keyCode: d.key_code ?? "",
      ctrl: d.ctrl === true,
      shift: d.shift === true,
      alt: d.alt === true,
      meta: d.meta === true,
    };
    dictInjectMethod = d.inject_method === "type" ? "type" : "paste";
    dictPressHoldMode = d.press_hold_mode === true;
    dictShortcutProfiles = (d.shortcut_profiles ?? [])
      .map(normalizeProfile)
      .filter((profile): profile is DictationShortcutProfile => profile !== null)
      .slice(0, MAX_DICTATION_SHORTCUT_PROFILES);
    persistedSettings = settingsSnapshot();
    publishSettingsState();
    hooks.setStatus(mainShortcut.keyCode ? `速记就绪，主快捷键：${comboLabel()}` : "速记就绪，当前未设置主快捷键。");
  } catch (error) {
    hooks.setStatus(`读取速记设置失败：${String(error)}`, "err");
  }
}
