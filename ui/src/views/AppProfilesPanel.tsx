import { useState } from "react";
import { ChevronDown, ChevronUp, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { Input, NumberInput, Select } from "@/components/ui/Input";
import { Checkbox } from "@/components/ui/Checkbox";
import { Field } from "@/components/ui/Field";
import { FormGrid } from "@/components/ui/FormGrid";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { Switch } from "@/components/ui/Switch";
import { RunningAppPicker } from "@/features/dictation/RunningAppPicker";
import { cn } from "@/lib/cn";
import {
  DEFAULT_SMART_PROCESSING_MIN_CHARS,
  MAX_APP_PROFILES,
  MAX_SMART_PROCESSING_MIN_CHARS,
  useDictPrefs,
  type AppProfile,
} from "@/store/useDictPrefs";

/** 三态覆盖项在下拉框里的取值；`inherit` 落库为 null。 */
const OVERRIDE_OPTIONS = [
  { value: "inherit", label: "跟随全局" },
  { value: "on", label: "开启" },
  { value: "off", label: "关闭" },
] as const;

function overrideValue(value: boolean | null): string {
  return value === null ? "inherit" : value ? "on" : "off";
}

function parseOverride(value: string): boolean | null {
  return value === "inherit" ? null : value === "on";
}

type SmartTriggerMode = "inherit" | "always" | "minimum";

function smartTriggerMode(value: number | null): SmartTriggerMode {
  return value === null ? "inherit" : value === 0 ? "always" : "minimum";
}

function newProfile(): AppProfile {
  return {
    id: crypto.randomUUID(),
    name: "",
    matchers: [],
    enabled: true,
    localRulesEnabled: null,
    smartProcessingEnabled: null,
    smartProcessingMinChars: null,
    smartTemplateId: null,
  };
}

export function AppProfilesPanel() {
  const prefs = useDictPrefs((s) => s.prefs);
  const patch = useDictPrefs((s) => s.patch);
  const profiles = prefs.appProfiles;

  const [editingId, setEditingId] = useState<string | null>(null);

  const updateProfile = (id: string, partial: Partial<AppProfile>) => {
    return patch({ appProfiles: profiles.map((p) => (p.id === id ? { ...p, ...partial } : p)) });
  };
  const moveProfile = (index: number, dir: -1 | 1) => {
    const target = index + dir;
    if (target < 0 || target >= profiles.length) return;
    const next = profiles.slice();
    [next[index], next[target]] = [next[target], next[index]];
    patch({ appProfiles: next });
  };
  const deleteProfile = (profile: AppProfile) => {
    const label = profile.name.trim() || profile.matchers[0] || "未命名规则";
    if (!window.confirm(`确定删除软件规则“${label}”吗？`)) return;
    patch({ appProfiles: profiles.filter((p) => p.id !== profile.id) });
    if (editingId === profile.id) setEditingId(null);
  };
  const addProfile = () => {
    if (profiles.length >= MAX_APP_PROFILES) return;
    const profile = newProfile();
    patch({ appProfiles: [...profiles, profile] });
    setEditingId(profile.id);
  };

  const summarize = (profile: AppProfile) => {
    const smart = profile.smartProcessingEnabled ?? prefs.smartProcessingEnabled;
    const smartMinChars = profile.smartProcessingMinChars ?? prefs.smartProcessingMinChars;
    const templateName = profile.smartTemplateId
      ? prefs.smartTemplates.find((t) => t.id === profile.smartTemplateId)?.name
      : null;
    const parts = [
      `本地处理${profile.localRulesEnabled === null ? "跟随全局" : profile.localRulesEnabled ? "开启" : "关闭"}`,
      `智能处理${profile.smartProcessingEnabled === null ? "跟随全局" : profile.smartProcessingEnabled ? "开启" : "关闭"}`,
    ];
    if (smart) {
      parts.push(smartMinChars === 0 ? "每次处理" : `达到 ${smartMinChars} 字符时处理`);
      parts.push(`模板：${templateName ?? "跟随全局"}`);
    }
    return parts.join(" · ");
  };

  return (
    <div className="flex flex-col gap-8">
      <SettingsSection
        title="软件规则"
        right={
          <Switch
            checked={prefs.appProfilesEnabled}
            onChange={(v) => patch({ appProfilesEnabled: v })}
            label="启用按软件规则"
          />
        }
      >
        <p className="max-w-[75ch] text-sm leading-relaxed text-[var(--color-fg-subtle)]">
          听写开始时识别当前软件，命中规则后按该规则的设置做后处理。规则从上往下匹配，取第一条命中的；
          没有命中的软件一律走全局配置。这里只读取软件名和窗口标题，不读取窗口内容，与场景感知的隐私黑名单无关。
        </p>
      </SettingsSection>

      <div
        className={cn(
          "flex flex-col gap-4 transition-opacity",
          !prefs.appProfilesEnabled && "pointer-events-none opacity-40",
        )}
      >
        <div className="overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-bg)]">
          {profiles.length === 0 && (
            <p className="px-3 py-2.5 text-xs text-[var(--color-fg-faint)]">
              暂无软件规则，添加后可为指定软件单独设置本地处理和智能处理。
            </p>
          )}
          {profiles.map((profile, index) => {
            const open = editingId === profile.id;
            const matcher = profile.matchers[0] ?? "";
            const smartActive = profile.smartProcessingEnabled ?? prefs.smartProcessingEnabled;
            const triggerMode = smartTriggerMode(profile.smartProcessingMinChars);
            return (
              <div key={profile.id} className="border-b border-[var(--color-line)] last:border-b-0">
                <div className="flex items-center gap-2.5 px-3 py-2">
                  <Checkbox
                    checked={profile.enabled}
                    onChange={(e) => updateProfile(profile.id, { enabled: e.target.checked })}
                    title={profile.enabled ? "已启用" : "已停用"}
                  />
                  <button
                    type="button"
                    onClick={() => setEditingId(open ? null : profile.id)}
                    aria-expanded={open}
                    className={cn(
                      "flex min-w-0 flex-1 flex-col gap-0.5 text-left",
                      profile.enabled ? "text-[var(--color-fg)]" : "text-[var(--color-fg-subtle)]",
                    )}
                  >
                    <span className="flex items-center gap-2 text-sm">
                      <span className="truncate">{profile.name || matcher || "（未指定软件）"}</span>
                      {!matcher && (
                        <span className="shrink-0 text-[11px] text-[var(--color-warn)]">● 未选择软件</span>
                      )}
                    </span>
                    <span className="truncate text-[11px] text-[var(--color-fg-faint)]">
                      {summarize(profile)}
                    </span>
                  </button>
                  <IconButton
                    size="sm"
                    className="h-7 w-7 shrink-0"
                    label="上移"
                    disabled={index === 0}
                    onClick={() => moveProfile(index, -1)}
                  >
                    <ChevronUp className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
                  </IconButton>
                  <IconButton
                    size="sm"
                    className="h-7 w-7 shrink-0"
                    label="下移"
                    disabled={index === profiles.length - 1}
                    onClick={() => moveProfile(index, 1)}
                  >
                    <ChevronDown className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
                  </IconButton>
                  <IconButton
                    size="sm"
                    variant="dangerHover"
                    className="h-7 w-7 shrink-0"
                    label="删除软件规则"
                    onClick={() => deleteProfile(profile)}
                  >
                    <Trash2 className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
                  </IconButton>
                </div>

                {open && (
                  <div className="border-t border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-4">
                    <FormGrid>
                      <RunningAppPicker
                        value={matcher}
                        label="软件"
                        hint="按进程名匹配，大小写不敏感。列表只显示当前打开的软件。"
                        onClear={() => updateProfile(profile.id, { matchers: [] })}
                        onSelect={(selection) =>
                          updateProfile(profile.id, {
                            matchers: [selection.processName],
                            // 名称留空时跟随所选软件，用户手动改过就不再覆盖。
                            name: profile.name.trim() === "" ? selection.appName : profile.name,
                          })
                        }
                      />

                      <Field label="规则名称" hint="仅用于在列表里辨认，可留空。">
                        <Input
                          value={profile.name}
                          placeholder={matcher || "例如：编程场景"}
                          spellCheck={false}
                          onChange={(e) => updateProfile(profile.id, { name: e.target.value })}
                        />
                      </Field>

                      <Field label="本地处理">
                        <Select
                          value={overrideValue(profile.localRulesEnabled)}
                          onChange={(e) =>
                            updateProfile(profile.id, {
                              localRulesEnabled: parseOverride(e.target.value),
                            })
                          }
                        >
                          {OVERRIDE_OPTIONS.map((option) => (
                            <option key={option.value} value={option.value}>
                              {option.label}
                            </option>
                          ))}
                        </Select>
                      </Field>

                      <Field label="智能处理">
                        <Select
                          value={overrideValue(profile.smartProcessingEnabled)}
                          onChange={(e) =>
                            updateProfile(profile.id, {
                              smartProcessingEnabled: parseOverride(e.target.value),
                            })
                          }
                        >
                          {OVERRIDE_OPTIONS.map((option) => (
                            <option key={option.value} value={option.value}>
                              {option.label}
                            </option>
                          ))}
                        </Select>
                      </Field>

                      <Field
                        label="处理时机"
                        hint={smartActive ? undefined : "智能处理未生效时该条件不会被使用。"}
                      >
                        <Select
                          disabled={!smartActive}
                          value={triggerMode}
                          onChange={(e) => {
                            const mode = e.target.value as SmartTriggerMode;
                            updateProfile(profile.id, {
                              smartProcessingMinChars:
                                mode === "inherit"
                                  ? null
                                  : mode === "always"
                                    ? 0
                                    : profile.smartProcessingMinChars
                                      || prefs.smartProcessingMinChars
                                      || DEFAULT_SMART_PROCESSING_MIN_CHARS,
                            });
                          }}
                        >
                          <option value="inherit">
                            跟随全局（{prefs.smartProcessingMinChars === 0
                              ? "每次听写"
                              : `达到 ${prefs.smartProcessingMinChars} 字符`}）
                          </option>
                          <option value="always">每次听写</option>
                          <option value="minimum">达到指定长度</option>
                        </Select>
                      </Field>

                      {triggerMode === "minimum" && (
                        <Field
                          label="最少文本长度"
                          hint={`达到 ${profile.smartProcessingMinChars} 个字符时触发。`}
                        >
                          <NumberInput
                            min={1}
                            max={MAX_SMART_PROCESSING_MIN_CHARS}
                            step={10}
                            disabled={!smartActive}
                            value={profile.smartProcessingMinChars || DEFAULT_SMART_PROCESSING_MIN_CHARS}
                            onValueChange={(value) =>
                              updateProfile(profile.id, { smartProcessingMinChars: value })
                            }
                          />
                        </Field>
                      )}

                      <Field
                        label="智能处理模板"
                        hint={smartActive ? undefined : "智能处理未生效时该模板不会被使用。"}
                      >
                        <Select
                          disabled={!smartActive}
                          value={profile.smartTemplateId ?? ""}
                          onChange={(e) =>
                            updateProfile(profile.id, {
                              smartTemplateId: e.target.value || null,
                            })
                          }
                        >
                          <option value="">跟随全局模板</option>
                          {prefs.smartTemplates.map((template) => (
                            <option key={template.id} value={template.id}>
                              {template.name}
                            </option>
                          ))}
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
          <Button size="sm" disabled={profiles.length >= MAX_APP_PROFILES} onClick={addProfile}>
            <Plus className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
            添加软件规则
          </Button>
        </div>
      </div>
    </div>
  );
}
