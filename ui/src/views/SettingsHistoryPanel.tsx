import { useEffect, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Field } from "@/components/ui/Field";
import { FormGrid } from "@/components/ui/FormGrid";
import { NumberInput, Textarea } from "@/components/ui/Input";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { Switch } from "@/components/ui/Switch";
import { CMD, cmd, type AppSnapshot } from "@/lib/tauri";

interface HistoryPrefs {
  enabled: boolean;
  retentionDays: number;
  excludedApps: string[];
}

const defaults: HistoryPrefs = { enabled: true, retentionDays: 30, excludedApps: [] };

function normalize(value: Record<string, unknown>): HistoryPrefs {
  return {
    enabled: value.enabled !== false,
    retentionDays: Math.min(3650, Math.max(1, Number(value.retentionDays) || 30)),
    excludedApps: Array.isArray(value.excludedApps) ? value.excludedApps.filter((item): item is string => typeof item === "string") : [],
  };
}

export function SettingsHistoryPanel() {
  const [prefs, setPrefs] = useState<HistoryPrefs>(defaults);
  const [excludedText, setExcludedText] = useState("");
  const [message, setMessage] = useState("");

  useEffect(() => {
    void cmd<AppSnapshot>(CMD.getAppSnapshot).then((snapshot) => {
      const next = normalize(snapshot.settings.historyPrefs);
      setPrefs(next);
      setExcludedText(next.excludedApps.join("\n"));
    }).catch((error) => setMessage(String(error)));
  }, []);

  async function save(next: HistoryPrefs) {
    setPrefs(next);
    try {
      await cmd(CMD.updateAppSettings, { domain: "history", value: next });
      setMessage("历史设置已保存");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function clear() {
    if (!window.confirm("确定清空全部本地历史和纠错样本吗？此操作不可撤销。")) return;
    try {
      await cmd(CMD.clearHistory);
      setMessage("本地历史和纠错样本已清空");
    } catch (error) {
      setMessage(String(error));
    }
  }

  return (
    <SettingsSection title="本地历史">
      <p className="text-xs text-[var(--color-fg-subtle)]">历史只保存在当前数据目录，不保存音频；安全输入框永不记录。</p>
      <FormGrid>
        <Field label="保存历史" hint="关闭后新任务不再写入，已有记录不会自动删除。">
          <Switch checked={prefs.enabled} onChange={(enabled) => void save({ ...prefs, enabled })} aria-label="保存历史" />
        </Field>
        <Field label="保留天数" hint="范围 1～3650 天，过期记录会自动清理。">
          <NumberInput value={prefs.retentionDays} min={1} max={3650} onValueChange={(retentionDays) => void save({ ...prefs, retentionDays })} aria-label="历史保留天数" />
        </Field>
        <Field label="排除应用" hint="每行一个进程名或应用名，不区分大小写。">
          <Textarea value={excludedText} onChange={(event) => setExcludedText(event.target.value)} onBlur={() => {
            const excludedApps = excludedText.split(/\r?\n/).map((item) => item.trim()).filter(Boolean);
            void save({ ...prefs, excludedApps });
          }} placeholder="例如：1Password.exe" aria-label="历史排除应用" />
        </Field>
        <Field label="清理数据" hint="立即删除全部历史及其纠错样本，此操作不可撤销。">
          <Button variant="dangerHover" onClick={() => void clear()}>清空全部历史</Button>
        </Field>
      </FormGrid>
      {message && <p role="status" className="text-xs text-[var(--color-fg-subtle)]">{message}</p>}
    </SettingsSection>
  );
}
