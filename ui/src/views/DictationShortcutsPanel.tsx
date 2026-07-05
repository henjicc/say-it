import { Field } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { Button } from "@/components/ui/Button";
import { FormGrid } from "@/components/ui/FormGrid";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { cn } from "@/lib/cn";
import { useDictationStore } from "@/store/useDictationStore";
import { useDictPrefs } from "@/store/useDictPrefs";
import { DICTATION_ASR_MODEL_OPTIONS } from "@/features/asr/modelOptions";
import { useAudioDevices } from "@/features/audio/devices";
import { startShortcutCapture, setInjectMethod } from "@/features/dictation/controller";

const DEFAULT_INPUT_VALUE = "";
const shortcutActionButtonClassName = "min-h-[var(--control-h)] shrink-0 self-stretch";

export function DictationShortcutsPanel() {
  const { capturing, shortcutLabel, injectMethod } = useDictationStore();
  const asrModel = useDictPrefs((s) => s.prefs.asrModel);
  const micDeviceId = useDictPrefs((s) => s.prefs.micDeviceId);
  const patchDictPrefs = useDictPrefs((s) => s.patch);
  const { inputs } = useAudioDevices();

  return (
    <div className="flex flex-col gap-8">
      <SettingsSection title="识别设置">
        <FormGrid>
          <Field label="识别模型">
            <Select value={asrModel} onChange={(e) => patchDictPrefs({ asrModel: e.target.value })}>
              {DICTATION_ASR_MODEL_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="声音来源">
            <Select
              searchable={inputs.length > 5}
              searchPlaceholder="搜索麦克风…"
              value={micDeviceId || DEFAULT_INPUT_VALUE}
              onChange={(e) => patchDictPrefs({ micDeviceId: e.target.value })}
            >
              <option value={DEFAULT_INPUT_VALUE}>默认输入</option>
              {inputs.map((device) => (
                <option key={device.name} value={device.name}>
                  {device.name}
                </option>
              ))}
            </Select>
          </Field>
        </FormGrid>
      </SettingsSection>

      <SettingsSection title="输入行为">
        <FormGrid>
          <Field label="全局快捷键">
            <div className="flex items-stretch gap-2">
              <Input
                readOnly
                value={capturing ? "请按下按键…" : shortcutLabel}
                placeholder="未设置"
                className={cn(capturing && "border-[var(--accent-ring)]")}
              />
              <Button className={shortcutActionButtonClassName} onClick={startShortcutCapture}>
                {capturing ? "取消" : "修改"}
              </Button>
            </div>
          </Field>
          <Field label="注入方式">
            <Select
              value={injectMethod}
              onChange={(e) => setInjectMethod(e.target.value as "paste" | "type")}
            >
              <option value="paste">剪贴板粘贴（推荐，适合长中文）</option>
              <option value="type">模拟逐字输入</option>
            </Select>
          </Field>
        </FormGrid>
        <p className="text-xs leading-relaxed text-[var(--color-fg-subtle)]">
          按下此快捷键开始/停止语音输入，过程中按 Esc 可取消。默认使用 Caps Lock（大写锁定键），
          用作语音键时不会切换大小写。点击「修改」后按下想用的按键即可。
        </p>
      </SettingsSection>
    </div>
  );
}
