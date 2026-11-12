import { useEffect, useState } from "react";
import { CircleAlert } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Group, Row, ROW_CONTROL_W, SliderRow } from "@/components/ui/Group";
import { Modal } from "@/components/ui/Modal";
import { Select } from "@/components/ui/Input";
import { Switch } from "@/components/ui/Switch";

import { Tabs } from "@/components/ui/Tabs";
import { DICTATION_ASR_MODEL_OPTIONS } from "@/features/asr/modelOptions";
import { modelInfo, useModelCatalogRevision } from "@/features/asr/modelRegistry";
import { ModelPicker } from "@/features/models/ModelPicker";
import { UsageLinkRow, useAsrUsageIssue, useOfferedAsrOptions, useUsageEditable } from "@/features/models/usage";
import { useAudioDevices } from "@/features/audio/devices";
import { setInjectMethod, setMainShortcut, setPressHoldMode } from "@/features/dictation/controller";
import { ShortcutRecorder } from "@/features/dictation/ShortcutRecorder";
import { useCustomizationStore } from "@/store/useCustomizationStore";
import { useDictationStore } from "@/store/useDictationStore";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useFloatingOrbStore } from "@/store/useFloatingOrbStore";
import { useMouseGestureStore } from "@/store/useMouseGestureStore";
import { useProviderStore } from "@/store/useProviderStore";
import { useUiStore } from "@/store/useUiStore";
import { FLOATING_ORB_OPACITY_RANGE, FLOATING_ORB_SIZE_PERCENT_RANGE } from "@/floating-orb/interaction";
import { CMD, cmd, type SetupStatus } from "@/lib/tauri";
import { EnvironmentStatus } from "@/views/SettingsSetupPanel";

const DEFAULT_INPUT_VALUE = "";
const DICTATION_SCENES = ["dictationRealtime", "dictationFile"];

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

/** label 是「用途」页里按功能名显示的行名，功能页里用默认名。 */
export function AsrModelRow({ label = "识别模型", hint }: { label?: string; hint?: string } = {}) {
  useModelCatalogRevision();
  const asrModel = useDictPrefs((s) => s.prefs.asrModel);
  const patch = useDictPrefs((s) => s.patch);
  const editable = useUsageEditable();
  const options = useOfferedAsrOptions(DICTATION_ASR_MODEL_OPTIONS, asrModel);
  const issue = useAsrUsageIssue(asrModel, DICTATION_SCENES);
  if (!editable) {
    return <UsageLinkRow label={label} value={options.find((option) => option.value === asrModel)?.label ?? asrModel} message={issue} />;
  }
  return (
    <Row label={label} hint={hint} controlId="dictation-asr-model" message={issue}>
      <div className={ROW_CONTROL_W}>
        <ModelPicker
          id="dictation-asr-model"
          value={asrModel}
          options={options}
          panelLabel="选择语音识别模型"
          onChange={(value) => patch({ asrModel: value })}
        />
      </div>
    </Row>
  );
}

/** 只有本地模型包识别需要补标点，其他模型不显示这一行。 */
export function LocalPunctuationRow() {
  useModelCatalogRevision();
  const asrModel = useDictPrefs((s) => s.prefs.asrModel);
  const enabled = useDictPrefs((s) => s.prefs.localPunctuationEnabled);
  const patch = useDictPrefs((s) => s.patch);
  const pack = useProviderStore((s) => s.profiles.find((profile) => profile.capabilities.includes("punctuation")));
  if (!modelInfo(asrModel)?.protocol.startsWith("local-sherpa")) return null;
  if (!pack?.enabled) {
    return (
      <Row label="自动加标点" hint="本地模型识别出的文字没有标点。安装并启用标点模型包后会自动补上。">
        <button type="button" onClick={() => useUiStore.getState().openModels("local")} className="rounded-[var(--radius-sm)] px-2 py-1 text-[13px] text-[var(--color-accent-text)] hover:bg-[var(--color-surface-hover)]">
          {pack ? "启用标点模型包" : "安装标点模型包"}
        </button>
      </Row>
    );
  }
  return (
    <Row label="自动加标点" hint="本地模型识别出的文字没有标点，开启后自动补上标点；这次语音输入会做智能处理时交给大模型整理，不再单独补。" controlId="dictation-local-punctuation">
      <Switch id="dictation-local-punctuation" checked={enabled} onChange={(checked) => patch({ localPunctuationEnabled: checked })} />
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

/** 基础设置按听写的先后顺序分组：触发 → 识别 → 输出。 */
export function BasicsSection() {
  const hotwordCount = useCustomizationStore((s) => s.prefs.hotwords.length);
  const openLexicon = () => useUiStore.setState({ view: "lexicon", lexiconSection: "hotwords" });
  return (
    <>
      <ReadinessGroup />
      <Group title="触发">
        <ShortcutRows />
      </Group>
      <Group title="识别">
        <MicRow />
        <AsrModelRow />
        <LocalPunctuationRow />
        <Row label="热词与上下文">
          <button type="button" onClick={openLexicon} className="rounded-[var(--radius-sm)] px-2 py-1 text-[13px] text-[var(--color-accent-text)] hover:bg-[var(--color-surface-hover)]">
            {hotwordCount ? `${hotwordCount} 个热词` : "去词库添加"}
          </button>
        </Row>
      </Group>
      <Group title="输出">
        <InjectMethodRow />
      </Group>
    </>
  );
}

/**
 * 听写现在用不了时的提示：环境检查加上听写所选识别模型的可用性，给出查看入口；一切正常时不占位置。
 * 已启用悬浮球或鼠标手势时，没设主快捷键不算待处理。
 */
function ReadinessGroup() {
  const [pending, setPending] = useState<number | null>(null);
  const [failed, setFailed] = useState(false);
  const [detailOpen, setDetailOpen] = useState(false);
  const asrModel = useDictPrefs((s) => s.prefs.asrModel);
  const modelIssue = useAsrUsageIssue(asrModel, DICTATION_SCENES);
  const orbEnabled = useFloatingOrbStore((state) => state.settings.enabled);
  const gestureEnabled = useMouseGestureStore((state) => state.settings.enabled);
  const hasOtherTrigger = orbEnabled || gestureEnabled;

  // 权限在系统设置里改；进入本页和关掉详情后各检查一次。
  useEffect(() => {
    if (detailOpen) return;
    let active = true;
    void cmd<SetupStatus>(CMD.getSetupStatus)
      .then((status) => {
        if (!active) return;
        setFailed(false);
        setPending(status.checks.filter((check) => check.status !== "ready" && !(check.id === "shortcut" && hasOtherTrigger)).length);
      })
      .catch(() => { if (active) setFailed(true); });
    return () => { active = false; };
  }, [detailOpen, hasOtherTrigger]);

  const ready = pending === 0 && !failed && !modelIssue;
  if ((pending === null && !failed) || ready) return null;
  return (
    <>
      <Group>
        <div className="flex min-h-[var(--row-h)] items-center gap-2.5 px-3.5">
          <CircleAlert aria-hidden className="h-4 w-4 flex-none text-[var(--color-warn)]" />
          <p role="status" className="min-w-0 flex-1 text-[13px] text-[var(--color-fg)]">
            {failed ? "环境检查失败" : pending ? `${pending} 项需要处理` : modelIssue}
          </p>
          {(failed || Boolean(pending)) && <Button size="sm" aria-haspopup="dialog" aria-label="查看环境状态" onClick={() => setDetailOpen(true)}>查看</Button>}
        </div>
      </Group>
      <Modal open={detailOpen} onClose={() => setDetailOpen(false)} title="环境状态" className="max-w-xl" bodyClassName="py-1">
        <EnvironmentStatus />
      </Modal>
    </>
  );
}

/** withAppearance：同时列出悬浮球的大小、不透明度和自动回车；首次引导只要开关。 */
export function OrbGestureSection({ withAppearance = false }: { withAppearance?: boolean }) {
  const floatingOrb = useFloatingOrbStore((state) => state.settings);
  const floatingOrbBusy = useFloatingOrbStore((state) => state.busy);
  const floatingOrbError = useFloatingOrbStore((state) => state.error);
  const setFloatingOrbEnabled = useFloatingOrbStore((state) => state.setEnabled);
  const setFloatingOrbAutoEnter = useFloatingOrbStore((state) => state.setAutoEnter);
  const updateFloatingOrbAppearance = useFloatingOrbStore((state) => state.updateAppearance);
  const orbOff = !floatingOrb.enabled;
  const mouseGesture = useMouseGestureStore((state) => state.settings);
  const mouseGestureBusy = useMouseGestureStore((state) => state.busy);
  const mouseGestureError = useMouseGestureStore((state) => state.error);
  const updateMouseGesture = useMouseGestureStore((state) => state.update);
  const gestureOff = !mouseGesture.enabled;

  return (
    <>
      <Group title="悬浮球">
        <Row
          label="启用悬浮球"
          hint="点击悬浮球开始或停止语音输入，右键也能调整这些设置。识别结果会保留在剪贴板。"
          message={floatingOrbError && <span role="alert" className="text-[var(--color-err)]">保存悬浮球设置失败：{floatingOrbError}</span>}
        >
          <Switch
            checked={floatingOrb.enabled}
            disabled={floatingOrbBusy}
            onChange={(enabled) => void setFloatingOrbEnabled(enabled).catch(() => undefined)}
            label="启用悬浮球"
          />
        </Row>
        {withAppearance && <>
        <SliderRow
          label="大小"
          min={FLOATING_ORB_SIZE_PERCENT_RANGE.min}
          max={FLOATING_ORB_SIZE_PERCENT_RANGE.max}
          step={1}
          value={floatingOrb.sizePercent}
          disabled={orbOff}
          format={(value) => `${(value / 10).toFixed(1)}%`}
          onChange={(sizePercent) => void updateFloatingOrbAppearance({ sizePercent }).catch(() => undefined)}
        />
        <SliderRow
          label="不透明度"
          min={FLOATING_ORB_OPACITY_RANGE.min}
          max={FLOATING_ORB_OPACITY_RANGE.max}
          step={1}
          value={floatingOrb.opacity}
          disabled={orbOff}
          format={(value) => `${value}%`}
          onChange={(opacity) => void updateFloatingOrbAppearance({ opacity }).catch(() => undefined)}
        />
        <Row label="自动回车" hint="识别完成后自动发送，悬浮球上不再显示回车图标。" disabled={orbOff}>
          <Switch
            checked={floatingOrb.autoEnter}
            disabled={orbOff || floatingOrbBusy}
            onChange={(enabled) => void setFloatingOrbAutoEnter(enabled).catch(() => undefined)}
            label="自动回车"
          />
        </Row>
        </>}
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
