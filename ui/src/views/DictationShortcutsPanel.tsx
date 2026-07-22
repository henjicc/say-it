import { Field } from "@/components/ui/Field";
import { Select } from "@/components/ui/Input";
import { FormGrid } from "@/components/ui/FormGrid";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { useDictationStore } from "@/store/useDictationStore";
import { useDictPrefs } from "@/store/useDictPrefs";
import { DICTATION_ASR_MODEL_OPTIONS } from "@/features/asr/modelOptions";
import { useModelCatalogRevision } from "@/features/asr/modelRegistry";
import { useAudioDevices } from "@/features/audio/devices";
import { setInjectMethod, setMainShortcut, setPressHoldMode } from "@/features/dictation/controller";
import { ShortcutRecorder } from "@/features/dictation/ShortcutRecorder";

const DEFAULT_INPUT_VALUE = "";
export function DictationShortcutsPanel() {
  useModelCatalogRevision();
  const { shortcut, injectMethod, pressHoldMode } = useDictationStore();
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
          <Field label="主快捷键">
            <ShortcutRecorder
              value={shortcut}
              onChange={setMainShortcut}
              onClear={() => setMainShortcut({ keyCode: "", ctrl: false, shift: false, alt: false, meta: false })}
            />
          </Field>
          <Field label="触发方式">
            <Select
              value={pressHoldMode ? "press-hold" : "toggle"}
              onChange={(e) => setPressHoldMode(e.target.value === "press-hold")}
            >
              <option value="toggle">单击切换</option>
              <option value="press-hold">长按说话</option>
            </Select>
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
          主快捷键跟随当前软件的场景规则。「单击切换」为按一次开始、再按一次结束；「长按说话」为按住开始、松手结束，Caps Lock
          短按仍保留系统大小写切换。过程中按 Esc 可取消。点击「录入」后按下想用的按键即可；点击输入框内的「×」可清除快捷键——
          清除后仍可使用快捷键方案，或在“语音输入”页手动开始。
        </p>
      </SettingsSection>
    </div>
  );
}
