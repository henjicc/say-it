import { useEffect, useRef, useState } from "react";
import { ChevronDown, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Checkbox } from "@/components/ui/Checkbox";
import { Field } from "@/components/ui/Field";
import { FormGrid } from "@/components/ui/FormGrid";
import { IconButton } from "@/components/ui/IconButton";
import { Input, NumberInput, Select } from "@/components/ui/Input";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { ShortcutRecorder } from "@/features/dictation/ShortcutRecorder";
import {
  MAX_DICTATION_SHORTCUT_PROFILES,
  shortcutLabel,
  shortcutSignature,
  updateShortcutProfiles,
  type DictationShortcutProfile,
  type ShortcutProcessingMode,
  type ShortcutTriggerMode,
} from "@/features/dictation/hotkeys";
import { cn } from "@/lib/cn";
import {
  DEFAULT_SMART_PROCESSING_MIN_CHARS,
  MAX_SMART_PROCESSING_MIN_CHARS,
  useDictPrefs,
} from "@/store/useDictPrefs";
import { useDictationStore } from "@/store/useDictationStore";

type SmartTriggerMode = "inherit" | "always" | "minimum";

const PROCESSING_MODES: Array<{ value: ShortcutProcessingMode; label: string }> = [
  { value: "followScene", label: "跟随场景" },
  { value: "raw", label: "原文输出" },
  { value: "localOnly", label: "仅本地处理" },
  { value: "smartOnly", label: "仅智能处理" },
  { value: "smartAndLocal", label: "智能处理后再本地处理" },
];

const SHORTCUT_TRIGGER_MODES: Array<{ value: ShortcutTriggerMode; label: string }> = [
  { value: "toggle", label: "单击切换" },
  { value: "pressHold", label: "长按说话" },
];

function triggerMode(value: number | null): SmartTriggerMode {
  return value === null ? "inherit" : value === 0 ? "always" : "minimum";
}

function createProfile(index: number): DictationShortcutProfile {
  return {
    id: crypto.randomUUID(),
    name: `快捷键方案 ${index}`,
    enabled: false,
    triggerMode: "toggle",
    keyCode: "",
    ctrl: false,
    shift: false,
    alt: false,
    meta: false,
    processingMode: "followScene",
    smartTemplateId: null,
    smartProcessingMinChars: null,
    injectMethod: null,
  };
}

function processingSummary(profile: DictationShortcutProfile): string {
  return PROCESSING_MODES.find((mode) => mode.value === profile.processingMode)?.label ?? "跟随场景";
}

function shortcutTriggerLabel(mode: ShortcutTriggerMode): string {
  return SHORTCUT_TRIGGER_MODES.find((item) => item.value === mode)?.label ?? "单击切换";
}

export function ShortcutProfilesPanel() {
  const prefs = useDictPrefs((state) => state.prefs);
  const mainShortcut = useDictationStore((state) => state.shortcut);
  const mainPressHoldMode = useDictationStore((state) => state.pressHoldMode);
  const profiles = useDictationStore((state) => state.shortcutProfiles);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draftNames, setDraftNames] = useState<Record<string, string>>({});
  const untouchedDraftIds = useRef(new Set<string>());

  useEffect(() => () => {
    const draftIds = new Set(untouchedDraftIds.current);
    if (draftIds.size === 0) return;
    const latest = useDictationStore.getState().shortcutProfiles;
    const next = latest.filter((profile) => !draftIds.has(profile.id));
    if (next.length !== latest.length) void updateShortcutProfiles(next).catch(() => {});
  }, []);

  const save = (next: DictationShortcutProfile[]) => {
    void updateShortcutProfiles(next).catch(() => {});
  };
  const update = (id: string, partial: Partial<DictationShortcutProfile>) => {
    untouchedDraftIds.current.delete(id);
    save(profiles.map((profile) => (profile.id === id ? { ...profile, ...partial } : profile)));
  };
  const remove = (profile: DictationShortcutProfile) => {
    if (!window.confirm(`确定删除快捷键方案“${profile.name}”吗？`)) return;
    untouchedDraftIds.current.delete(profile.id);
    save(profiles.filter((item) => item.id !== profile.id));
    if (editingId === profile.id) setEditingId(null);
  };
  const add = () => {
    if (profiles.length >= MAX_DICTATION_SHORTCUT_PROFILES) return;
    const profile = createProfile(profiles.length + 1);
    untouchedDraftIds.current.add(profile.id);
    save([...profiles, profile]);
    setEditingId(profile.id);
  };

  const conflictMessage = (profile: DictationShortcutProfile): string => {
    if (!profile.keyCode) return "尚未设置快捷键，录入后会自动启用。";
    const signature = shortcutSignature(profile);
    const mainTriggerMode: ShortcutTriggerMode = mainPressHoldMode ? "pressHold" : "toggle";
    if (
      mainShortcut.keyCode
      && shortcutSignature(mainShortcut) === signature
      && profile.triggerMode === mainTriggerMode
    ) return `与主快捷键的${shortcutTriggerLabel(mainTriggerMode)}冲突`;
    const duplicate = profiles.find(
      (candidate) => candidate.id !== profile.id
        && candidate.keyCode
        && candidate.triggerMode === profile.triggerMode
        && shortcutSignature(candidate) === signature,
    );
    return duplicate ? `与“${duplicate.name}”的${shortcutTriggerLabel(profile.triggerMode)}冲突` : "";
  };

  return (
    <div className="flex flex-col gap-6">
      <SettingsSection title="快捷键方案">
        <p className="max-w-[75ch] text-sm leading-relaxed text-[var(--color-fg-subtle)]">
          为临时意图设置专用快捷键。听写开始时会冻结对应方案；录音过程中修改设置或按下其他听写快捷键，
          都不会切换当前文本的处理方式。同一按键可分别设置一条单击和一条长按方案，相同触发方式则会提示冲突。
        </p>
      </SettingsSection>

      <div className="overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-bg)]">
        {profiles.length === 0 && (
          <p className="px-3 py-3 text-xs text-[var(--color-fg-faint)]">
            暂无快捷键方案。主快捷键仍会按当前软件的场景规则处理。
          </p>
        )}
        {profiles.map((profile) => {
          const open = editingId === profile.id;
          const conflict = conflictMessage(profile);
          const smartRelevant = profile.processingMode === "followScene"
            || profile.processingMode === "smartOnly"
            || profile.processingMode === "smartAndLocal";
          const mode = triggerMode(profile.smartProcessingMinChars);
          return (
            <div key={profile.id} className="border-b border-[var(--color-line)] last:border-b-0">
              <div className="flex items-center gap-2.5 px-3 py-2.5">
                <Checkbox
                  checked={profile.enabled}
                  disabled={!profile.keyCode || Boolean(conflict)}
                  onChange={(event) => update(profile.id, { enabled: event.target.checked })}
                  title={profile.enabled ? "已启用" : "已停用"}
                />
                <button
                  type="button"
                  className="min-w-0 flex-1 text-left"
                  aria-expanded={open}
                  onClick={() => setEditingId(open ? null : profile.id)}
                >
                  <span className="flex items-center gap-2 text-sm text-[var(--color-fg)]">
                    <span className="truncate">{profile.name}</span>
                    {conflict && <span className="text-[11px] text-[var(--color-warn)]">{conflict}</span>}
                  </span>
                  <span className="block truncate text-[11px] text-[var(--color-fg-faint)]">
                    {shortcutLabel(profile) || "未设置"} · {shortcutTriggerLabel(profile.triggerMode)} · {processingSummary(profile)}
                  </span>
                </button>
                <IconButton
                  size="sm"
                  className="h-7 w-7 shrink-0"
                  label={open ? "收起" : "展开"}
                  onClick={() => setEditingId(open ? null : profile.id)}
                >
                  <ChevronDown className={cn("h-3.5 w-3.5 transition-transform", open && "rotate-180")} aria-hidden />
                </IconButton>
                <IconButton
                  size="sm"
                  variant="dangerHover"
                  className="h-7 w-7 shrink-0"
                  label="删除快捷键方案"
                  onClick={() => remove(profile)}
                >
                  <Trash2 className="h-3.5 w-3.5" aria-hidden />
                </IconButton>
              </div>

              {open && (
                <div className="border-t border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-4">
                  <FormGrid>
                    <Field label="方案名称">
                      <Input
                        value={draftNames[profile.id] ?? profile.name}
                        maxLength={80}
                        onChange={(event) => setDraftNames((current) => ({
                          ...current,
                          [profile.id]: event.target.value,
                        }))}
                        onBlur={() => {
                          const name = (draftNames[profile.id] ?? profile.name).trim();
                          setDraftNames((current) => {
                            const next = { ...current };
                            delete next[profile.id];
                            return next;
                          });
                          if (name && name !== profile.name) update(profile.id, { name });
                        }}
                      />
                    </Field>
                    <Field label="快捷键" hint={conflict || undefined}>
                      <ShortcutRecorder
                        value={profile}
                        onChange={(shortcut) => update(profile.id, { ...shortcut, enabled: true })}
                        onClear={() => update(profile.id, {
                          keyCode: "",
                          ctrl: false,
                          shift: false,
                          alt: false,
                          meta: false,
                          enabled: false,
                        })}
                      />
                    </Field>
                    <Field label="触发方式">
                      <Select
                        value={profile.triggerMode}
                        onChange={(event) => update(profile.id, {
                          triggerMode: event.target.value as ShortcutTriggerMode,
                        })}
                      >
                        {SHORTCUT_TRIGGER_MODES.map((item) => (
                          <option key={item.value} value={item.value}>{item.label}</option>
                        ))}
                      </Select>
                    </Field>
                    <Field label="处理方式">
                      <Select
                        value={profile.processingMode}
                        onChange={(event) => {
                          const processingMode = event.target.value as ShortcutProcessingMode;
                          update(profile.id, {
                            processingMode,
                            smartProcessingMinChars:
                              (processingMode === "smartOnly" || processingMode === "smartAndLocal")
                              && profile.smartProcessingMinChars === null
                                ? 0
                                : profile.smartProcessingMinChars,
                          });
                        }}
                      >
                        {PROCESSING_MODES.map((item) => (
                          <option key={item.value} value={item.value}>{item.label}</option>
                        ))}
                      </Select>
                    </Field>

                    {smartRelevant && (
                      <>
                        <Field
                          label="智能处理时机"
                          hint={profile.processingMode === "followScene"
                            ? "仅在当前场景最终启用智能处理时生效。"
                            : undefined}
                        >
                          <Select
                            value={mode}
                            onChange={(event) => {
                              const nextMode = event.target.value as SmartTriggerMode;
                              update(profile.id, {
                                smartProcessingMinChars: nextMode === "inherit"
                                  ? null
                                  : nextMode === "always"
                                    ? 0
                                    : profile.smartProcessingMinChars
                                      || prefs.smartProcessingMinChars
                                      || DEFAULT_SMART_PROCESSING_MIN_CHARS,
                              });
                            }}
                          >
                            <option value="inherit">跟随场景</option>
                            <option value="always">每次听写</option>
                            <option value="minimum">达到指定长度</option>
                          </Select>
                        </Field>
                        {mode === "minimum" && (
                          <Field label="最少文本长度">
                            <NumberInput
                              min={1}
                              max={MAX_SMART_PROCESSING_MIN_CHARS}
                              step={10}
                              value={profile.smartProcessingMinChars || DEFAULT_SMART_PROCESSING_MIN_CHARS}
                              onValueChange={(value) => update(profile.id, { smartProcessingMinChars: value })}
                            />
                          </Field>
                        )}
                        <Field label="智能处理模板">
                          <Select
                            value={profile.smartTemplateId ?? ""}
                            onChange={(event) => update(profile.id, { smartTemplateId: event.target.value || null })}
                          >
                            <option value="">跟随场景</option>
                            {prefs.smartTemplates.map((template) => (
                              <option key={template.id} value={template.id}>{template.name}</option>
                            ))}
                          </Select>
                        </Field>
                      </>
                    )}

                    <Field label="注入方式">
                      <Select
                        value={profile.injectMethod ?? ""}
                        onChange={(event) => update(profile.id, {
                          injectMethod: event.target.value === "paste" || event.target.value === "type"
                            ? event.target.value
                            : null,
                        })}
                      >
                        <option value="">继承全局</option>
                        <option value="paste">剪贴板粘贴</option>
                        <option value="type">模拟逐字输入</option>
                      </Select>
                    </Field>
                  </FormGrid>
                </div>
              )}
            </div>
          );
        })}
      </div>

      <div>
        <Button size="sm" disabled={profiles.length >= MAX_DICTATION_SHORTCUT_PROFILES} onClick={add}>
          <Plus className="h-3.5 w-3.5" aria-hidden />
          添加快捷键方案
        </Button>
      </div>
    </div>
  );
}
