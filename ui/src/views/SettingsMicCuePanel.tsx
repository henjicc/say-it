import { useRef } from "react";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Group, Row, ROW_CONTROL_W } from "@/components/ui/Group";
import { Input, Select } from "@/components/ui/Input";
import { Switch } from "@/components/ui/Switch";
import { useDictPrefs, type CueKind } from "@/store/useDictPrefs";
import { playCue } from "@/lib/cues";
import { useAudioDevices } from "@/features/audio/devices";
import { CMD, cmd } from "@/lib/tauri";

const DEFAULT_INPUT_VALUE = "";

const CUE_OPTIONS: { value: CueKind; label: string }[] = [
  { value: "none", label: "无" },
  { value: "beep-up", label: "内置·升调" },
  { value: "beep-down", label: "内置·降调" },
  { value: "beep-double", label: "内置·双响" },
  { value: "custom", label: "自定义文件…" },
];

/**
 * 提示音字段：音色下拉 + 试听。
 * 选中「自定义文件…」后额外给出「更换」——原生 select 重选同一项不触发 onChange，
 * 没有这个入口就无法替换已选文件。
 */
function CueRow({
  label,
  value,
  onSelect,
  onPickFile,
  onPreview,
}: {
  label: string;
  value: CueKind;
  onSelect: (value: CueKind) => void;
  onPickFile: () => void;
  onPreview: () => void;
}) {
  return (
    <Row label={label}>
      {value === "custom" && <Button size="sm" onClick={onPickFile}>更换</Button>}
      <Button size="sm" onClick={onPreview}>试听</Button>
      <Select className="w-40" aria-label={label} value={value} onChange={(e) => onSelect(e.target.value as CueKind)}>
        {CUE_OPTIONS.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </Select>
    </Row>
  );
}

export function SettingsMicCuePanel() {
  const prefs = useDictPrefs((s) => s.prefs);
  const patch = useDictPrefs((s) => s.patch);
  const { inputs } = useAudioDevices();
  const cueTargetRef = useRef<"start" | "end" | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

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
        playCue(target);
      } finally {
        cueTargetRef.current = null;
      }
    };
    reader.readAsDataURL(file);
  };

  const onCueSelect = (which: "start" | "end", value: CueKind) => {
    patch(which === "start" ? { cueStart: value } : { cueEnd: value });
    if (value === "custom") pickCueFile(which);
  };

  return (
    <InspectorBody>
      <Group title="麦克风保活">
        <Row label="输入设备">
          <Select
            className={ROW_CONTROL_W}
            aria-label="输入设备"
            searchable={inputs.length > 5}
            searchPlaceholder="搜索麦克风…"
            value={prefs.micDeviceId || DEFAULT_INPUT_VALUE}
            onChange={(e) => patch({ micDeviceId: e.target.value })}
          >
            <option value={DEFAULT_INPUT_VALUE}>默认输入</option>
            {inputs.map((device) => (
              <option key={device.name} value={device.name}>
                {device.name}
              </option>
            ))}
          </Select>
        </Row>
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
        <CueRow
          label="开始提示音"
          value={prefs.cueStart}
          onSelect={(v) => onCueSelect("start", v)}
          onPickFile={() => pickCueFile("start")}
          onPreview={() => playCue("start")}
        />
        <CueRow
          label="结束提示音"
          value={prefs.cueEnd}
          onSelect={(v) => onCueSelect("end", v)}
          onPickFile={() => pickCueFile("end")}
          onPreview={() => playCue("end")}
        />
      </Group>
      <input ref={fileRef} type="file" accept="audio/*" className="hidden" onChange={onCueFile} />
    </InspectorBody>
  );
}
