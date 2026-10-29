import { Group, Row, ROW_CONTROL_W, SliderRow } from "@/components/ui/Group";
import { Select } from "@/components/ui/Input";
import { Switch } from "@/components/ui/Switch";

import { Tabs } from "@/components/ui/Tabs";
import { DICTATION_ASR_MODEL_OPTIONS } from "@/features/asr/modelOptions";
import { useModelCatalogRevision } from "@/features/asr/modelRegistry";
import { ModelPicker } from "@/features/models/ModelPicker";
import { useAudioDevices } from "@/features/audio/devices";
import { setInjectMethod, setMainShortcut, setPressHoldMode } from "@/features/dictation/controller";
import { ShortcutRecorder } from "@/features/dictation/ShortcutRecorder";
import { useCustomizationStore } from "@/store/useCustomizationStore";
import { useDictationStore } from "@/store/useDictationStore";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useFloatingOrbStore } from "@/store/useFloatingOrbStore";
import { useMouseGestureStore } from "@/store/useMouseGestureStore";
import { useUiStore } from "@/store/useUiStore";

const DEFAULT_INPUT_VALUE = "";

/** 主快捷键 + 触发方式，两种模式共用。 */
export function ShortcutRows() {
  const { shortcut, pressHoldMode } = useDictationStore();
  return (
    <>
      <Row label="快捷键" hint="点击后按下想用的按键；按 Esc 取消录制，聚焦后按 Delete 清除。Caps Lock 短按仍可切换大小写。">
        <div className={ROW_CONTROL_W}>
          <ShortcutRecorder
            value={shortcut}
            onChange={setMainShortcut}
            onClear={() => setMainShortcut({ keyCode: "", ctrl: false, shift: false, alt: false, meta: false })}
          />
        </div>
      </Row>
      <Row label="触发方式" hint="按一下：按一次开始，再按一次结束；按住：按住说话，松开结束。">
        <Tabs
          variant="view"
          ariaLabel="触发方式"
          tabs={[{ key: "toggle", label: "按一下" }, { key: "hold", label: "按住" }]}
          active={pressHoldMode ? "hold" : "toggle"}
          onChange={(key) => setPressHoldMode(key === "hold")}
        />
      </Row>
    </>
  );
}

export function MicRow({ label = "麦克风" }: { label?: string }) {
  const micDeviceId = useDictPrefs((s) => s.prefs.micDeviceId);
  const patch = useDictPrefs((s) => s.patch);
  const { inputs } = useAudioDevices();
  return (
    <Row label={label} hint="语音输入和实时字幕的麦克风共用这一设置。">
      <Select
        className={ROW_CONTROL_W}
        aria-label={label}
        searchable={inputs.length > 5}
        searchPlaceholder="搜索麦克风…"
        value={micDeviceId || DEFAULT_INPUT_VALUE}
        onChange={(e) => patch({ micDeviceId: e.target.value })}
      >
        <option value={DEFAULT_INPUT_VALUE}>系统默认</option>
        {inputs.map((device) => (
          <option key={device.name} value={device.name}>{device.name}</option>
        ))}
      </Select>
    </Row>
  );
}

export function AsrModelRow() {
  useModelCatalogRevision();
  const asrModel = useDictPrefs((s) => s.prefs.asrModel);
  const patch = useDictPrefs((s) => s.patch);
  return (
    <Row label="识别模型" controlId="dictation-asr-model">
      <div className={ROW_CONTROL_W}>
        <ModelPicker
          id="dictation-asr-model"
          value={asrModel}
          options={DICTATION_ASR_MODEL_OPTIONS}
          panelLabel="选择语音识别模型"
          onChange={(value) => patch({ asrModel: value })}
        />
      </div>
    </Row>
  );
}

export function InjectMethodRow() {
  const injectMethod = useDictationStore((s) => s.injectMethod);
  return (
    <Row label="输入方式" hint="剪贴板粘贴适合长中文；个别软件不支持粘贴时改用逐字输入。">
      <Tabs
        variant="view"
        ariaLabel="输入方式"
        tabs={[{ key: "paste", label: "粘贴" }, { key: "type", label: "逐字输入" }]}
        active={injectMethod}
        onChange={(key) => setInjectMethod(key as "paste" | "type")}
      />
    </Row>
  );
}

export function TriggerSection() {
  const floatingOrb = useFloatingOrbStore((state) => state.settings);
  const floatingOrbBusy = useFloatingOrbStore((state) => state.busy);
  const floatingOrbError = useFloatingOrbStore((state) => state.error);
  const setFloatingOrbEnabled = useFloatingOrbStore((state) => state.setEnabled);
  const mouseGesture = useMouseGestureStore((state) => state.settings);
  const mouseGestureBusy = useMouseGestureStore((state) => state.busy);
  const mouseGestureError = useMouseGestureStore((state) => state.error);
  const updateMouseGesture = useMouseGestureStore((state) => state.update);
  const gestureOff = !mouseGesture.enabled;

  return (
    <>
      <Group>
        <ShortcutRows />
      </Group>
      <Group title="悬浮球">
        <Row
          label="启用悬浮球"
          hint="点击悬浮球开始或停止语音输入，右键可调整外观。识别结果会保留在剪贴板。"
          message={floatingOrbError && <span role="alert" className="text-[var(--color-err)]">保存悬浮球设置失败：{floatingOrbError}</span>}
        >
          <Switch
            checked={floatingOrb.enabled}
            disabled={floatingOrbBusy}
            onChange={(enabled) => void setFloatingOrbEnabled(enabled).catch(() => undefined)}
            label="启用悬浮球"
          />
        </Row>
      </Group>
      <Group title="鼠标手势">
        <Row
          label="启用鼠标手势"
          message={mouseGestureError && <span role="alert" className="text-[var(--color-err)]">鼠标手势不可用：{mouseGestureError}</span>}
        >
          <Switch
            checked={mouseGesture.enabled}
            disabled={mouseGestureBusy}
            onChange={(enabled) => void updateMouseGesture({ enabled }).catch(() => undefined)}
            label="启用鼠标手势"
          />
        </Row>
        <Row label="晃动后" disabled={gestureOff}>
          <Select
            className={ROW_CONTROL_W}
            aria-label="晃动后"
            value={mouseGesture.mode}
            disabled={gestureOff}
            onChange={(event) => void updateMouseGesture({ mode: event.target.value as "confirm" | "direct" }).catch(() => undefined)}
          >
            <option value="confirm">显示悬浮球，点击后开始</option>
            <option value="direct">直接开始或停止</option>
          </Select>
        </Row>
        <SliderRow
          label="晃动灵敏度"
          hint="数值越低越不容易误触发。"
          min={0}
          max={100}
          step={5}
          value={mouseGesture.sensitivity}
          disabled={gestureOff}
          format={(value) => `${value}%`}
          onChange={(sensitivity) => void updateMouseGesture({ sensitivity }).catch(() => undefined)}
        />
        <Row label="左键快速连击" disabled={gestureOff}>
          <Select
            className="w-32"
            aria-label="连击次数"
            value={String(mouseGesture.rapidClickCount)}
            disabled={gestureOff || !mouseGesture.rapidClickEnabled}
            onChange={(event) => void updateMouseGesture({ rapidClickCount: Number(event.target.value) }).catch(() => undefined)}
          >
            {[3, 4, 5, 6, 7, 8, 9, 10].map((count) => <option key={count} value={String(count)}>{`连击 ${count} 次`}</option>)}
          </Select>
          <Switch
            checked={mouseGesture.rapidClickEnabled}
            disabled={gestureOff}
            onChange={(rapidClickEnabled) => void updateMouseGesture({ rapidClickEnabled }).catch(() => undefined)}
            label="启用左键快速连击"
          />
        </Row>
      </Group>
    </>
  );
}

export function AsrSection() {
  const hotwordCount = useCustomizationStore((s) => s.prefs.hotwords.length);
  const openLexicon = () => useUiStore.setState({ view: "lexicon", lexiconSection: "hotwords" });
  return (
    <>
      <Group>
        <AsrModelRow />
        <Row label="热词与上下文">
          <button type="button" onClick={openLexicon} className="rounded-[var(--radius-sm)] px-2 py-1 text-[13px] text-[var(--color-accent-light)] hover:bg-[var(--color-surface-hover)]">
            {hotwordCount ? `${hotwordCount} 个热词` : "去词库添加"}
          </button>
        </Row>
      </Group>
    </>
  );
}
