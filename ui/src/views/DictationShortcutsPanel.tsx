import { Field } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";
import { useDictationStore } from "@/store/useDictationStore";
import { useDictPrefs } from "@/store/useDictPrefs";
import { REALTIME_ASR_MODEL_OPTIONS } from "@/features/asr/modelOptions";
import { useAudioDevices } from "@/features/audio/devices";
import { startShortcutCapture, setInjectMethod } from "@/features/dictation/controller";

const DEFAULT_INPUT_VALUE = "";

export function DictationShortcutsPanel() {
  const { capturing, shortcutLabel, injectMethod } = useDictationStore();
  const asrModel = useDictPrefs((s) => s.prefs.asrModel);
  const micDeviceId = useDictPrefs((s) => s.prefs.micDeviceId);
  const patchDictPrefs = useDictPrefs((s) => s.patch);
  const { inputs } = useAudioDevices();

  return (
    <>
      <div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
        <div className="flex flex-col gap-3">
          <Field label="识别模型">
            <Select value={asrModel} onChange={(e) => patchDictPrefs({ asrModel: e.target.value })}>
              {REALTIME_ASR_MODEL_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="全局快捷键">
            <div className="flex gap-2">
              <Input
                readOnly
                value={capturing ? "请按下按键…" : shortcutLabel}
                placeholder="未设置"
                className={cn(capturing && "border-white/40")}
              />
              <Button className="shrink-0" onClick={startShortcutCapture}>
                {capturing ? "取消" : "修改"}
              </Button>
            </div>
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
          <Field label="注入方式">
            <Select
              value={injectMethod}
              onChange={(e) => setInjectMethod(e.target.value as "paste" | "type")}
            >
              <option value="paste">剪贴板粘贴（推荐，适合长中文）</option>
              <option value="type">模拟逐字输入</option>
            </Select>
          </Field>
        </div>
      </div>
      <p className="mt-2 text-xs text-white/40">
        点击「修改」后按下想用的按键即可（按 Esc 取消）。默认使用 Caps Lock（大写锁定键），
        用作语音键时不会切换大小写。
      </p>
    </>
  );
}
