import { CMD, cmd } from "@/lib/tauri";
import { loadDictationSettings } from "@/features/dictation/controller";
import { loadSubtitleShortcut } from "@/features/subtitles/controller";
import type { ShortcutCombo, ShortcutTriggerMode } from "@/features/dictation/hotkeys";

export type ShortcutTarget =
  | { kind: "dictationMain" }
  | { kind: "dictationProfile"; profileId: string }
  | { kind: "subtitles" };

export interface ShortcutBindingItem extends ShortcutCombo {
  target: ShortcutTarget;
  name: string;
  actionLabel: string;
  enabled: boolean;
  triggerMode: ShortcutTriggerMode;
  triggerModeEditable: boolean;
}

export function shortcutTargetKey(target: ShortcutTarget): string {
  return target.kind === "dictationProfile"
    ? `${target.kind}:${target.profileId}`
    : target.kind;
}

export async function loadShortcutBindings(): Promise<ShortcutBindingItem[]> {
  return cmd<ShortcutBindingItem[]>(CMD.getShortcutBindings);
}

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
  await syncShortcutDomain(item.target);
  return items;
}

export async function clearShortcutBinding(item: ShortcutBindingItem): Promise<ShortcutBindingItem[]> {
  const items = await cmd<ShortcutBindingItem[]>(CMD.clearShortcutBinding, { target: item.target });
  await syncShortcutDomain(item.target);
  return items;
}

async function syncShortcutDomain(target: ShortcutTarget) {
  if (target.kind === "subtitles") {
    await loadSubtitleShortcut();
  } else {
    await loadDictationSettings();
  }
}
