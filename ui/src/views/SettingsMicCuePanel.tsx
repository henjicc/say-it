import { useRef, useState } from "react";
import { Check, Play } from "lucide-react";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Group, Row, ROW_CONTROL_W } from "@/components/ui/Group";
import { IconButton } from "@/components/ui/IconButton";
import { Input, Select } from "@/components/ui/Input";
import { Modal } from "@/components/ui/Modal";
import { Switch } from "@/components/ui/Switch";
import { cn } from "@/lib/cn";
import { useDictPrefs, type CueKind } from "@/store/useDictPrefs";
import { CUE_DESIGNS, cueLabel, previewCue, type CueSlot } from "@/lib/cues";
import { CMD, cmd } from "@/lib/tauri";

const SLOT_LABEL: Record<CueSlot, string> = { start: "开始", end: "结束" };
const HOLD_OPTIONS = [
  { value: 1, label: "1 分钟" },
  { value: 5, label: "5 分钟" },
  { value: 10, label: "10 分钟" },
  { value: 30, label: "30 分钟" },
  { value: 60, label: "1 小时" },
  { value: 0, label: "一直保持" },
];

/** 音色表里的一格：左边试听，右边选中。 */
function CueCell({
  slot,
  kind,
  name,
  selected,
  onSelect,
  action,
  canPreview = true,
}: {
  slot: CueSlot;
  kind: CueKind;
  name: string;
  selected: boolean;
  onSelect: () => void;
  /** 替换「选择」按钮的文字，如自定义文件的「选择文件…」。 */
  action?: string;
  canPreview?: boolean;
}) {
  return (
    <div
      className={cn(
        "flex items-center gap-1 rounded-[var(--radius-md)] border p-1",
        selected ? "border-[var(--color-accent)] bg-[var(--accent-soft)]" : "border-transparent",
      )}
    >
      <IconButton
        size="sm"
        label={`试听${SLOT_LABEL[slot]}音：${name}`}
        disabled={!canPreview}
        onClick={() => previewCue(kind, slot)}
      >
        <Play size={14} />
      </IconButton>
      <Button
        size="sm"
        aria-pressed={action ? undefined : selected}
        aria-label={action ? `${SLOT_LABEL[slot]}提示音：${action}` : `选为${SLOT_LABEL[slot]}提示音：${name}`}
        className={cn("flex-1 justify-center", selected && !action && "text-[var(--color-accent-text)]")}
        onClick={onSelect}
      >
        {action ?? (selected ? <Check size={14} /> : "选择")}
      </Button>
    </div>
  );
}

/** 音色选择窗：一行一套音色，开始、结束两列各自试听和选中。 */
function CuePicker({
  open,
  onClose,
  start,
  end,
  onSelect,
  onPickFile,
}: {
  open: boolean;
  onClose: () => void;
  start: CueKind;
  end: CueKind;
  onSelect: (slot: CueSlot, kind: CueKind) => void;
  onPickFile: (slot: CueSlot) => void;
}) {
  const current = { start, end };
  const rows: { name: string; start: CueKind; end: CueKind }[] = [
    ...CUE_DESIGNS,
    { name: "无", start: "none", end: "none" },
  ];
  return (
    <Modal open={open} onClose={onClose} title="选择提示音" className="max-w-xl" bodyClassName="p-3">
      <div className="grid grid-cols-[minmax(0,1fr)_9rem_9rem] items-center gap-x-2 gap-y-1">
        <span />
        <span className="px-1 text-xs text-[var(--color-fg-muted)]">开始</span>
        <span className="px-1 text-xs text-[var(--color-fg-muted)]">结束</span>
        {rows.map((row) => (
          <div key={row.name} className="contents">
            <span className="truncate px-2 text-[13px] text-[var(--color-fg)]">{row.name}</span>
            {(["start", "end"] as const).map((slot) =>
              row[slot] === "none" ? (
                <Button
                  key={slot}
                  size="sm"
                  aria-pressed={current[slot] === "none"}
                  className={cn(current[slot] === "none" && "border-[var(--color-accent)] text-[var(--color-accent-text)]")}
                  onClick={() => onSelect(slot, "none")}
                >
                  {current[slot] === "none" ? <Check size={14} /> : "不播放"}
                </Button>
              ) : (
                <CueCell
                  key={slot}
                  slot={slot}
                  kind={row[slot]}
                  name={row.name}
                  selected={current[slot] === row[slot]}
                  onSelect={() => onSelect(slot, row[slot])}
                />
              ),
            )}
          </div>
        ))}
        <span className="truncate px-2 text-[13px] text-[var(--color-fg)]">自定义文件</span>
        {(["start", "end"] as const).map((slot) => (
          <CueCell
            key={slot}
            slot={slot}
            kind="custom"
            name="自定义文件"
            selected={current[slot] === "custom"}
            canPreview={current[slot] === "custom"}
            action={current[slot] === "custom" ? "更换…" : "选择文件…"}
            onSelect={() => onPickFile(slot)}
          />
        ))}
      </div>
    </Modal>
  );
}

export function SettingsMicCuePanel() {
  const prefs = useDictPrefs((s) => s.prefs);
  const patch = useDictPrefs((s) => s.patch);
  const cueTargetRef = useRef<"start" | "end" | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const [pickerOpen, setPickerOpen] = useState(false);

  const pickCueFile = (which: "start" | "end") => {
    cueTargetRef.current = which;
    if (fileRef.current) {
      fileRef.current.value = "";
      fileRef.current.click();
    }
  };
  const onCueFile = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    const target = cueTargetRef.current;
    if (!file || !target) return;
    const reader = new FileReader();
    reader.onload = async () => {
      const dataUrl = String(reader.result || "");
      const key = target === "start" ? "dictCueStartData" : "dictCueEndData";
      try {
        await cmd(CMD.updateCustomCue, { which: target, dataUrl });
        localStorage.setItem(key, dataUrl);
        await patch(target === "start" ? { cueStart: "custom" } : { cueEnd: "custom" });
        previewCue("custom", target);
      } finally {
        cueTargetRef.current = null;
      }
    };
    reader.readAsDataURL(file);
  };

  const onCueSelect = (which: CueSlot, value: CueKind) => {
    void patch(which === "start" ? { cueStart: value } : { cueEnd: value });
    previewCue(value, which);
  };

  return (
    <InspectorBody>
      <Group title="麦克风">
        <Row label="麦克风保活" hint="单位为秒；0 表示用完即关。" controlId="mic-keep-alive">
          <Input
            id="mic-keep-alive"
            className="w-32"
            type="number"
            min={0}
            max={600}
            step={5}
            value={Math.round((prefs.keepAliveMs || 0) / 1000)}
            onChange={(e) =>
              patch({
                keepAliveMs: Math.max(0, Math.min(600, Number(e.target.value) || 0)) * 1000,
              })
            }
          />
        </Row>
      </Group>

      <Group title="提示音">
        <Row label="启用音频提示" hint="开始和结束语音输入时播放提示音。">
          <Switch checked={prefs.cueEnabled} onChange={(v) => patch({ cueEnabled: v })} label="启用音频提示" />
        </Row>
        <Row label="提示音" hint="听写开始和结束时各播放哪种声音。">
          <span className="text-[13px] text-[var(--color-fg-muted)]">
            {cueLabel(prefs.cueStart)} / {cueLabel(prefs.cueEnd)}
          </span>
          <Button size="sm" onClick={() => setPickerOpen(true)}>
            选择…
          </Button>
        </Row>
        <Row
          label="保持音频输出"
          hint="两次听写之间不关闭音频输出，部分音箱就不会在提示音前后发出呲啦声。保持期间系统会认为一直在播放声音。"
        >
          <Switch checked={prefs.cueHoldEnabled} onChange={(v) => patch({ cueHoldEnabled: v })} label="保持音频输出" />
        </Row>
        <Row
          label="保持时长"
          hint="最后一次提示音之后再保持多久。"
          disabled={!prefs.cueHoldEnabled}
          message={
            prefs.cueHoldEnabled && prefs.cueHoldMinutes === 0 ? (
              <span className="text-[var(--color-fg-muted)]">一直保持时，系统不会自动睡眠，蓝牙耳机也不会待机。</span>
            ) : undefined
          }
        >
          <Select
            className="w-40"
            aria-label="保持时长"
            disabled={!prefs.cueHoldEnabled}
            value={String(prefs.cueHoldMinutes)}
            onChange={(e) => patch({ cueHoldMinutes: Number(e.target.value) })}
          >
            {HOLD_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </Select>
        </Row>
      </Group>
      <CuePicker
        open={pickerOpen}
        onClose={() => setPickerOpen(false)}
        start={prefs.cueStart}
        end={prefs.cueEnd}
        onSelect={onCueSelect}
        onPickFile={pickCueFile}
      />
      <input ref={fileRef} type="file" accept="audio/*" className="hidden" onChange={onCueFile} />
    </InspectorBody>
  );
}
