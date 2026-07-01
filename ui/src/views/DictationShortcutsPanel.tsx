import { Field } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";
import { useDictationStore } from "@/store/useDictationStore";
import { startShortcutCapture, setInjectMethod } from "@/features/dictation/controller";

export function DictationShortcutsPanel() {
  const { capturing, shortcutLabel, injectMethod } = useDictationStore();

  return (
    <>
      <div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
        <Field label="全局快捷键">
          <div className="flex gap-2">
            <Input
              readOnly
              value={capturing ? "请按下按键…" : shortcutLabel}
              placeholder="未设置"
              className={cn(capturing && "border-white/40")}
            />
            <Button onClick={startShortcutCapture}>{capturing ? "取消" : "设置快捷键"}</Button>
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
      </div>
      <p className="mt-2 text-xs text-white/40">
        点击「设置快捷键」后按下想用的按键即可（按 Esc 取消）。默认使用 Caps Lock（大写锁定键），
        用作语音键时不会切换大小写。
      </p>
    </>
  );
}
