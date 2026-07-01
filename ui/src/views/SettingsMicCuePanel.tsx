import { useRef } from "react";
import { Card, CardTitle } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { Field, CheckField } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { useDictPrefs, type CueKind } from "@/store/useDictPrefs";
import { playCue } from "@/lib/cues";

const CUE_OPTIONS: { value: CueKind; label: string }[] = [
  { value: "none", label: "无" },
  { value: "beep-up", label: "内置·升调" },
  { value: "beep-down", label: "内置·降调" },
  { value: "beep-double", label: "内置·双响" },
  { value: "custom", label: "自定义文件…" },
];

export function SettingsMicCuePanel() {
  const prefs = useDictPrefs((s) => s.prefs);
  const patch = useDictPrefs((s) => s.patch);
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
    reader.onload = () => {
      const dataUrl = String(reader.result || "");
      const key = target === "start" ? "dictCueStartData" : "dictCueEndData";
      try {
        localStorage.setItem(key, dataUrl);
      } catch {
        return;
      }
      patch(target === "start" ? { cueStart: "custom" } : { cueEnd: "custom" });
      playCue(target);
      cueTargetRef.current = null;
    };
    reader.readAsDataURL(file);
  };

  const onCueSelect = (which: "start" | "end", value: CueKind) => {
    patch(which === "start" ? { cueStart: value } : { cueEnd: value });
    if (value === "custom") pickCueFile(which);
  };

  return (
    <Card>
      <CardTitle>麦克风保活与提示音</CardTitle>
      <div className="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2">
        <Field label="麦克风保活（秒，0=用完即关）">
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
      </div>

      <div className="mt-4">
        <CheckField checked={prefs.cueEnabled} onChange={(v) => patch({ cueEnabled: v })}>
          启用音频提示
        </CheckField>
      </div>

      <div className="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2">
        <Field label="开始提示音">
          <Select value={prefs.cueStart} onChange={(e) => onCueSelect("start", e.target.value as CueKind)}>
            {CUE_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="结束提示音">
          <Select value={prefs.cueEnd} onChange={(e) => onCueSelect("end", e.target.value as CueKind)}>
            {CUE_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </Select>
        </Field>
      </div>

      <div className="mt-3 flex flex-wrap gap-2">
        <Button size="sm" onClick={() => pickCueFile("start")}>
          选择开始音文件
        </Button>
        <Button size="sm" onClick={() => pickCueFile("end")}>
          选择结束音文件
        </Button>
        <Button size="sm" onClick={() => playCue("start")}>
          试听开始音
        </Button>
        <Button size="sm" onClick={() => playCue("end")}>
          试听结束音
        </Button>
      </div>
      <input ref={fileRef} type="file" accept="audio/*" className="hidden" onChange={onCueFile} />
      <p className="mt-3 text-xs text-white/40">
        增益 / 降噪参数请在「设置 → 音频调教」中调试，调好后会自动应用到语音输入。
      </p>
    </Card>
  );
}
