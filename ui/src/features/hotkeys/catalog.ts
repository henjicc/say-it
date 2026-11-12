import { CMD, cmd } from "@/lib/tauri";
import { loadDictationSettings } from "@/features/dictation/controller";
import { loadSubtitleShortcut } from "@/features/subtitles/controller";
import type { ShortcutCombo, ShortcutTriggerMode } from "@/features/dictation/hotkeys";
import { setQueryData, type Query } from "@/hooks/useCachedQuery";

export type ShortcutTarget =
  | { kind: "dictationMain" }
  | { kind: "dictationProfile"; profileId: string }
  | { kind: "subtitles" }
  | { kind: "assistant"; action: "translateSpeech" | "editSelection" | "ask" };

export interface ShortcutBindingItem extends ShortcutCombo {
  target: ShortcutTarget;
  name: string;
  actionLabel: string;
  enabled: boolean;
  triggerMode: ShortcutTriggerMode;
  triggerModeEditable: boolean;
  /** 这条绑定当前没能真正注册进系统时的原因；正常时为 null。 */
  registrationError?: string | null;
}

export function shortcutTargetKey(target: ShortcutTarget): string {
  return target.kind === "dictationProfile"
    ? `${target.kind}:${target.profileId}`
    : target.kind === "assistant" ? `${target.kind}:${target.action}` : target.kind;
}

export async function loadShortcutBindings(): Promise<ShortcutBindingItem[]> {
  return cmd<ShortcutBindingItem[]>(CMD.getShortcutBindings);
}

/** 全部快捷键，各页共用一份；改动后用命令返回的列表更新。 */
export const shortcutBindingsQuery: Query<ShortcutBindingItem[]> = {
  key: "shortcutBindings",
  load: loadShortcutBindings,
};

export async function updateShortcutBinding(
  item: ShortcutBindingItem,
  shortcut: ShortcutCombo,
  triggerMode: ShortcutTriggerMode,
): Promise<ShortcutBindingItem[]> {
  const items = await cmd<ShortcutBindingItem[]>(CMD.updateShortcutBinding, {
    target: item.target,
    binding: {
      keyCode: shortcut.keyCode,
      ctrl: shortcut.ctrl,
      shift: shortcut.shift,
      alt: shortcut.alt,
      meta: shortcut.meta,
      triggerMode,
    },
  });
  setQueryData(shortcutBindingsQuery, items);
  await syncShortcutDomain(item.target);
  return items;
}

export async function clearShortcutBinding(item: ShortcutBindingItem): Promise<ShortcutBindingItem[]> {
  const items = await cmd<ShortcutBindingItem[]>(CMD.clearShortcutBinding, { target: item.target });
  setQueryData(shortcutBindingsQuery, items);
  await syncShortcutDomain(item.target);
  return items;
}

async function syncShortcutDomain(target: ShortcutTarget) {
  if (target.kind === "subtitles") {
    await loadSubtitleShortcut();
  } else if (target.kind !== "assistant") {
    await loadDictationSettings();
  }
}
