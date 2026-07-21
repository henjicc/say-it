import { useRef } from "react";
import { Button } from "@/components/ui/Button";
import { Field } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { Switch } from "@/components/ui/Switch";
import { FormGrid } from "@/components/ui/FormGrid";
import { SettingsSection } from "@/components/ui/SettingsSection";
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
function CueField({
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
    <Field
      label={label}
      actions={
        <>
          <Button onClick={onPreview}>试听</Button>
          {value === "custom" && <Button onClick={onPickFile}>更换</Button>}
        </>
      }
    >
      <Select value={value} onChange={(e) => onSelect(e.target.value as CueKind)}>
        {CUE_OPTIONS.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </Select>
    </Field>
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
    <div className="flex flex-col gap-8">
      <SettingsSection title="麦克风保活">
        <FormGrid>
          <Field label="输入设备">
            <Select
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
          </Field>
          <Field label="麦克风保活" hint="单位为秒；0 表示用完即关。">
            <Input
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
          </Field>
        </FormGrid>
      </SettingsSection>

      <SettingsSection title="提示音">
        <div className="flex items-center gap-4 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3.5">
          <div className="min-w-0 flex-1">
            <p className="text-sm font-medium text-[var(--color-fg)]">启用音频提示</p>
            <p className="mt-0.5 text-xs leading-relaxed text-[var(--color-fg-subtle)]">
              语音输入开始与结束时播放提示音。
            </p>
          </div>
          <Switch checked={prefs.cueEnabled} onChange={(v) => patch({ cueEnabled: v })} label="启用音频提示" />
        </div>

        <FormGrid>
          <CueField
            label="开始提示音"
            value={prefs.cueStart}
            onSelect={(v) => onCueSelect("start", v)}
            onPickFile={() => pickCueFile("start")}
            onPreview={() => playCue("start")}
          />
          <CueField
            label="结束提示音"
            value={prefs.cueEnd}
            onSelect={(v) => onCueSelect("end", v)}
            onPickFile={() => pickCueFile("end")}
            onPreview={() => playCue("end")}
          />
        </FormGrid>
        <input ref={fileRef} type="file" accept="audio/*" className="hidden" onChange={onCueFile} />
      </SettingsSection>
    </div>
  );
}
