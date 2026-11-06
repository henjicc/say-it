import { useEffect, useMemo, useState } from "react";
import { ArrowRight, Trash2 } from "lucide-react";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Group, Row } from "@/components/ui/Group";
import { IconButton } from "@/components/ui/IconButton";
import { Select } from "@/components/ui/Input";
import { useConfirm } from "@/components/ui/useConfirm";
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
  const { confirm, dialog } = useConfirm();
  const [items, setItems] = useState<ShortcutBindingItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const openDictation = useUiStore((state) => state.openDictationShortcutSettings);
  const openAssistant = useUiStore((state) => state.openAssistantSettings);
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
    dictation: items.filter((item) => item.target.kind === "dictationMain" || item.target.kind === "dictationProfile"),
    assistant: items.filter((item) => item.target.kind === "assistant"),
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
      if (!reportShortcutConflict(error)) {
        setErrors((current) => ({ ...current, [key]: String(error) }));
      }
      await refresh().catch(() => {});
    } finally {
      setBusyKey(null);
    }
  };

  const changeShortcut = (item: ShortcutBindingItem, shortcut: ShortcutCombo) =>
    runMutation(item, () => updateShortcutBinding(item, shortcut, item.triggerMode));

  const changeTrigger = (item: ShortcutBindingItem, triggerMode: ShortcutTriggerMode) =>
    runMutation(item, () => updateShortcutBinding(item, item, triggerMode));

  const clear = async (item: ShortcutBindingItem) => {
    if (
      !(await confirm({
        title: "确认清除快捷键",
        message: `确定清除“${item.name}”的快捷键吗？对应功能配置会继续保留。`,
        confirmLabel: "清除",
      }))
    ) {
      return;
    }
    void runMutation(item, () => clearShortcutBinding(item));
  };

  const goToSource = (item: ShortcutBindingItem) => {
    if (item.target.kind === "subtitles") {
      openSubtitles();
    } else if (item.target.kind === "assistant") {
      openAssistant(item.target.action);
    } else {
      openDictation(item.target.kind === "dictationProfile" ? item.target.profileId : undefined);
    }
  };

  const renderGroup = (title: string, groupItems: ShortcutBindingItem[]) => {
    if (groupItems.length === 0) return null;
    return (
      <Group title={title}>
        {groupItems.map((item) => {
          const key = shortcutTargetKey(item.target);
          return (
            <Row
              key={key}
              label={
                <span className="flex min-w-0 items-center gap-2">
                  <span className="truncate">{item.name}</span>
                  <span className="truncate text-xs text-[var(--color-fg-subtle)]">{item.actionLabel}</span>
                  {!item.enabled && (
                    <span className="flex-none rounded-[var(--radius-control)] bg-[var(--color-surface-strong)] px-2 py-0.5 text-[11px] text-[var(--color-fg-subtle)]">
                      已停用
                    </span>
                  )}
                </span>
              }
              // 注册失败时界面必须说出来：否则这里照样显示成已绑定，用户按了没反应
              // 也无从得知（启动时的失败原本只写进 release 版根本没有的控制台）。
              message={(item.registrationError || errors[key]) && (
                <span role="alert" className="flex flex-col gap-1 text-[var(--color-err)]">
                  {item.registrationError && <span>当前未生效：{item.registrationError}</span>}
                  {errors[key] && <span>{errors[key]}</span>}
                </span>
              )}
            >
              <div className="w-44">
                <ShortcutRecorder
                  value={item}
                  disabled={mutating}
                  ariaLabel={`${item.name}的快捷键`}
                  onChange={(shortcut) => changeShortcut(item, shortcut)}
                />
              </div>
              {item.triggerModeEditable ? (
                <Select
                  className="w-32"
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
                <span className="w-32 px-3 text-[13px] text-[var(--color-fg-subtle)]">单击切换（固定）</span>
              )}
              <IconButton
                size="sm"
                disabled={mutating}
                label={`前往 ${item.name} 的设置`}
                onClick={() => goToSource(item)}
              >
                <ArrowRight className="h-3.5 w-3.5" aria-hidden />
              </IconButton>
              <IconButton
                size="sm"
                variant="dangerHover"
                disabled={mutating}
                label={`清除 ${item.name} 的快捷键`}
                onClick={() => void clear(item)}
              >
                <Trash2 className="h-3.5 w-3.5" aria-hidden />
              </IconButton>
            </Row>
          );
        })}
      </Group>
    );
  };

  if (loading) {
    return <p className="text-sm text-[var(--color-fg-subtle)]">正在读取按键设置…</p>;
  }

  if (items.length === 0) {
    return (
      <Group>
        {errors.page ? (
          <Row
            label="读取按键设置失败"
            message={<span role="alert" className="text-[var(--color-err)]">{errors.page}</span>}
          >
            <Button
              size="sm"
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
          </Row>
        ) : (
          <p className="px-3.5 py-3 text-[13px] text-[var(--color-fg-subtle)]">当前没有已设置的快捷键。</p>
        )}
      </Group>
    );
  }

  return (
    <InspectorBody>
      {renderGroup("语音输入", groups.dictation)}
      {renderGroup("智能助手", groups.assistant)}
      {renderGroup("实时字幕", groups.subtitles)}
      {dialog}
    </InspectorBody>
  );
}
