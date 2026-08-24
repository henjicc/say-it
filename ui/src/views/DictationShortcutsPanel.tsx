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
import { Switch } from "@/components/ui/Switch";
import { useFloatingOrbStore } from "@/store/useFloatingOrbStore";
import { useMouseGestureStore } from "@/store/useMouseGestureStore";
import { Slider } from "@/components/ui/Slider";

const DEFAULT_INPUT_VALUE = "";
export function DictationShortcutsPanel() {
  useModelCatalogRevision();
  const { shortcut, injectMethod, pressHoldMode } = useDictationStore();
  const asrModel = useDictPrefs((s) => s.prefs.asrModel);
  const micDeviceId = useDictPrefs((s) => s.prefs.micDeviceId);
  const patchDictPrefs = useDictPrefs((s) => s.patch);
  const { inputs } = useAudioDevices();
  const floatingOrb = useFloatingOrbStore((state) => state.settings);
  const floatingOrbBusy = useFloatingOrbStore((state) => state.busy);
  const floatingOrbError = useFloatingOrbStore((state) => state.error);
  const setFloatingOrbEnabled = useFloatingOrbStore((state) => state.setEnabled);
  const mouseGesture = useMouseGestureStore((state) => state.settings);
  const mouseGestureBusy = useMouseGestureStore((state) => state.busy);
  const mouseGestureError = useMouseGestureStore((state) => state.error);
  const updateMouseGesture = useMouseGestureStore((state) => state.update);

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
          <Field label="主快捷键触发方式">
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

      <SettingsSection title="悬浮球">
        <div className="flex items-start justify-between gap-6">
          <div className="min-w-0">
            <p className="text-sm font-medium text-[var(--color-fg)]">启用悬浮球输入</p>
            <p className="mt-1 max-w-[75ch] text-xs leading-relaxed text-[var(--color-fg-subtle)]">
              点击开始或停止语音输入，右键调整悬浮球；识别结果会保留在剪贴板。
            </p>
          </div>
          <Switch
            checked={floatingOrb.enabled}
            disabled={floatingOrbBusy}
            onChange={(enabled) => void setFloatingOrbEnabled(enabled).catch(() => undefined)}
            aria-label="启用悬浮球输入"
          />
        </div>
        {floatingOrbError && (
          <p className="text-xs text-[var(--color-err)]" role="alert">
            保存悬浮球设置失败：{floatingOrbError}
          </p>
        )}
      </SettingsSection>

      <SettingsSection title="鼠标手势">
        <div className="flex items-start justify-between gap-6">
          <p className="text-sm font-medium text-[var(--color-fg)]">启用鼠标手势</p>
          <Switch
            checked={mouseGesture.enabled}
            disabled={mouseGestureBusy}
            onChange={(enabled) => void updateMouseGesture({ enabled }).catch(() => undefined)}
            label="启用鼠标手势"
          />
        </div>
        <FormGrid className={!mouseGesture.enabled ? "opacity-50" : undefined}>
          <Field label="触发方式">
            <Select
              value={mouseGesture.mode}
              disabled={!mouseGesture.enabled}
              onChange={(event) => void updateMouseGesture({ mode: event.target.value as "confirm" | "direct" }).catch(() => undefined)}
            >
              <option value="confirm">晃动后点击确认（推荐）</option>
              <option value="direct">晃动直接开始或停止</option>
            </Select>
          </Field>
          <Field label="触发灵敏度">
            <Slider
              label="低误触"
              min={0}
              max={100}
              step={5}
              value={mouseGesture.sensitivity}
              disabled={!mouseGesture.enabled}
              format={(value) => `${value}%`}
              onChange={(sensitivity) => void updateMouseGesture({ sensitivity }).catch(() => undefined)}
            />
          </Field>
        </FormGrid>
        {mouseGestureError && (
          <p className="text-xs text-[var(--color-err)]" role="alert">
            鼠标手势不可用：{mouseGestureError}
          </p>
        )}
      </SettingsSection>
    </div>
  );
}
