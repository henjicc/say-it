import { useEffect, useMemo, useState } from "react";
import { ArrowRight, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { Select } from "@/components/ui/Input";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { ShortcutRecorder } from "@/features/dictation/ShortcutRecorder";
import type { ShortcutCombo, ShortcutTriggerMode } from "@/features/dictation/hotkeys";
import {
  clearShortcutBinding,
  loadShortcutBindings,
  shortcutTargetKey,
  updateShortcutBinding,
  type ShortcutBindingItem,
} from "@/features/hotkeys/catalog";
import { reportShortcutConflict } from "@/features/hotkeys/conflictFeedback";
import { useUiStore } from "@/store/useUiStore";

const TRIGGER_OPTIONS: Array<{ value: ShortcutTriggerMode; label: string }> = [
  { value: "toggle", label: "单击切换" },
  { value: "pressHold", label: "长按说话" },
];

export function SettingsKeyBindingsPanel() {
  const [items, setItems] = useState<ShortcutBindingItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const openDictation = useUiStore((state) => state.openDictationShortcutSettings);
  const openSubtitles = useUiStore((state) => state.openSubtitleShortcutSettings);

  const refresh = async () => {
    const next = await loadShortcutBindings();
    setItems(next);
  };

  useEffect(() => {
    let cancelled = false;
    loadShortcutBindings()
      .then((next) => {
        if (!cancelled) setItems(next);
      })
      .catch((error) => {
        if (!cancelled) setErrors({ page: String(error) });
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const groups = useMemo(() => ({
    dictation: items.filter((item) => item.target.kind !== "subtitles"),
    subtitles: items.filter((item) => item.target.kind === "subtitles"),
  }), [items]);
  const mutating = busyKey !== null;

  const runMutation = async (
    item: ShortcutBindingItem,
    operation: () => Promise<ShortcutBindingItem[]>,
  ) => {
    const key = shortcutTargetKey(item.target);
    setBusyKey(key);
    setErrors((current) => ({ ...current, [key]: "" }));
    try {
      setItems(await operation());
    } catch (error) {
      reportShortcutConflict(error);
      setErrors((current) => ({ ...current, [key]: String(error) }));
      await refresh().catch(() => {});
    } finally {
      setBusyKey(null);
    }
  };

  const changeShortcut = (item: ShortcutBindingItem, shortcut: ShortcutCombo) =>
    runMutation(item, () => updateShortcutBinding(item, shortcut, item.triggerMode));

  const changeTrigger = (item: ShortcutBindingItem, triggerMode: ShortcutTriggerMode) =>
    runMutation(item, () => updateShortcutBinding(item, item, triggerMode));

  const clear = (item: ShortcutBindingItem) => {
    if (!window.confirm(`确定清除“${item.name}”的快捷键吗？对应功能配置会继续保留。`)) return;
    void runMutation(item, () => clearShortcutBinding(item));
  };

  const goToSource = (item: ShortcutBindingItem) => {
    if (item.target.kind === "subtitles") {
      openSubtitles();
    } else {
      openDictation(item.target.kind === "dictationProfile" ? item.target.profileId : undefined);
    }
  };

  const renderGroup = (title: string, groupItems: ShortcutBindingItem[]) => {
    if (groupItems.length === 0) return null;
    return (
      <SettingsSection title={title}>
        <div className="overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-bg)]">
          {groupItems.map((item) => {
            const key = shortcutTargetKey(item.target);
            return (
              <div key={key} className="border-b border-[var(--color-line)] p-4 last:border-b-0">
                <div className="flex items-start justify-between gap-4">
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <p className="truncate text-sm font-medium text-[var(--color-fg)]">{item.name}</p>
                      {!item.enabled && (
                        <span className="rounded-[var(--radius-control)] bg-[var(--color-surface-strong)] px-2 py-0.5 text-[11px] text-[var(--color-fg-subtle)]">
                          已停用
                        </span>
                      )}
                    </div>
                    <p className="mt-1 text-xs text-[var(--color-fg-subtle)]">{item.actionLabel}</p>
                  </div>
                  <div className="flex shrink-0 items-center gap-1.5">
                    <Button size="sm" variant="ghost" disabled={mutating} onClick={() => goToSource(item)}>
                      前往设置
                      <ArrowRight className="h-3.5 w-3.5" aria-hidden />
                    </Button>
                    <IconButton
                      size="sm"
                      variant="dangerHover"
                      className="h-7 w-7"
                      disabled={mutating}
                      label={`清除 ${item.name} 的快捷键`}
                      onClick={() => clear(item)}
                    >
                      <Trash2 className="h-3.5 w-3.5" aria-hidden />
                    </IconButton>
                  </div>
                </div>

                <div className="mt-3 grid gap-3 md:grid-cols-[minmax(0,1fr)_minmax(180px,0.42fr)]">
                  <ShortcutRecorder
                    value={item}
                    disabled={mutating}
                    ariaLabel={`${item.name}的快捷键`}
                    onChange={(shortcut) => changeShortcut(item, shortcut)}
                  />
                  {item.triggerModeEditable ? (
                    <Select
                      value={item.triggerMode}
                      disabled={mutating}
                      aria-label={`${item.name}的触发方式`}
                      onChange={(event) => void changeTrigger(item, event.target.value as ShortcutTriggerMode)}
                    >
                      {TRIGGER_OPTIONS.map((option) => (
                        <option key={option.value} value={option.value}>{option.label}</option>
                      ))}
                    </Select>
                  ) : (
                    <div className="flex min-h-[var(--control-h)] items-center rounded-[var(--radius-control)] border border-[var(--color-line)] px-3 text-sm text-[var(--color-fg-subtle)]">
                      单击切换（固定）
                    </div>
                  )}
                </div>
                {errors[key] && (
                  <p className="mt-2 text-xs text-[var(--color-err)]" role="alert">{errors[key]}</p>
                )}
              </div>
            );
          })}
        </div>
      </SettingsSection>
    );
  };

  if (loading) {
    return <p className="text-sm text-[var(--color-fg-subtle)]">正在读取按键设置…</p>;
  }

  if (items.length === 0) {
    return (
      <SettingsSection title="按键">
        {errors.page ? (
          <>
            <p className="text-sm text-[var(--color-err)]" role="alert">读取按键设置失败：{errors.page}</p>
            <Button
              size="sm"
              className="mt-3"
              onClick={() => {
                setErrors({});
                setLoading(true);
                void refresh()
                  .catch((error) => setErrors({ page: String(error) }))
                  .finally(() => setLoading(false));
              }}
            >
              重新读取
            </Button>
          </>
        ) : (
          <p className="text-sm text-[var(--color-fg-subtle)]">当前没有已设置的快捷键。</p>
        )}
      </SettingsSection>
    );
  }

  return (
    <div className="flex flex-col gap-7">
      <SettingsSection title="集中管理">
        <p className="max-w-[75ch] text-sm leading-relaxed text-[var(--color-fg-subtle)]">
          在这里修改或清除所有已绑定的快捷键。处理方式、模板和方案启用状态请前往对应功能页面调整。
        </p>
      </SettingsSection>
      {renderGroup("语音输入", groups.dictation)}
      {renderGroup("实时字幕", groups.subtitles)}
    </div>
  );
}
