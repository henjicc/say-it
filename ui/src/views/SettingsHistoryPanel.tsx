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
  finalDraftObservationEnabled: boolean;
  correctionLearningEnabled: boolean;
  cloudLearningContextEnabled: boolean;
  learningMemoryRetentionDays: number;
  retentionDays: number;
  excludedApps: string[];
}

const defaults: HistoryPrefs = {
  enabled: true,
  finalDraftObservationEnabled: false,
  correctionLearningEnabled: false,
  cloudLearningContextEnabled: false,
  learningMemoryRetentionDays: 180,
  retentionDays: 30,
  excludedApps: [],
};

function normalize(value: Record<string, unknown>): HistoryPrefs {
  return {
    enabled: value.enabled !== false,
    finalDraftObservationEnabled: value.finalDraftObservationEnabled === true || value.finalDraftLearningEnabled === true,
    correctionLearningEnabled: value.correctionLearningEnabled === true || value.finalDraftLearningEnabled === true,
    cloudLearningContextEnabled: value.cloudLearningContextEnabled === true,
    learningMemoryRetentionDays: Math.min(3650, Math.max(1, Number(value.learningMemoryRetentionDays) || 180)),
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

  async function clearUsage() {
    if (!window.confirm("确定清空本地累计使用统计吗？历史记录不会受到影响。")) return;
    try {
      await cmd(CMD.clearUsageSummary);
      setMessage("本地累计使用统计已清空");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function clearLearning() {
    if (!window.confirm("确定清空全部个性化学习记忆吗？历史正文会保留，此操作不可撤销。")) return;
    try {
      await cmd(CMD.clearLearningMemory);
      setMessage("个性化学习记忆已清空");
    } catch (error) {
      setMessage(String(error));
    }
  }

  function setCloudLearningContext(enabled: boolean) {
    if (enabled && !window.confirm("开启后，云端智能处理会接收最多三条与当前文本直接相关、已脱敏的局部纠错，以及一条已确认的表达偏好。是否继续？")) {
      return;
    }
    void save({ ...prefs, cloudLearningContextEnabled: enabled });
  }

  return (
    <SettingsSection title="本地历史">
      <p className="text-xs text-[var(--color-fg-subtle)]">历史只保存在当前数据目录，不保存音频；安全输入框永不记录。</p>
      <FormGrid>
        <Field label="保存历史" controlId="history-enabled" hint="关闭后新任务不再写入，已有记录不会自动删除。">
          <Switch id="history-enabled" checked={prefs.enabled} onChange={(enabled) => void save({ ...prefs, enabled })} label="保存历史" />
        </Field>
        <Field label="记录发送前修改" controlId="history-final-draft-observation" hint="仅限本次听写对应输入框，最长观察 120 秒；不会常驻采集键盘或其他文本。">
          <Switch
            id="history-final-draft-observation"
            checked={prefs.finalDraftObservationEnabled}
            disabled={!prefs.enabled}
            onChange={(finalDraftObservationEnabled) => void save({
              ...prefs,
              finalDraftObservationEnabled,
              correctionLearningEnabled: finalDraftObservationEnabled ? prefs.correctionLearningEnabled : false,
              cloudLearningContextEnabled: finalDraftObservationEnabled ? prefs.cloudLearningContextEnabled : false,
            })}
            label="记录发送前修改"
          />
        </Field>
        <Field label="个性化纠错" controlId="history-correction-learning" hint="局部纠错重复出现两次或经你确认后，才会成为本地规则。">
          <Switch
            id="history-correction-learning"
            checked={prefs.correctionLearningEnabled}
            disabled={!prefs.enabled || !prefs.finalDraftObservationEnabled}
            onChange={(correctionLearningEnabled) => void save({
              ...prefs,
              correctionLearningEnabled,
              cloudLearningContextEnabled: correctionLearningEnabled ? prefs.cloudLearningContextEnabled : false,
            })}
            label="个性化纠错"
          />
        </Field>
        <Field label="云端参考学习记录" controlId="history-cloud-learning" hint="开启后，云端智能处理最多接收三条脱敏的相关局部示例；默认关闭。">
          <Switch
            id="history-cloud-learning"
            checked={prefs.cloudLearningContextEnabled}
            disabled={!prefs.correctionLearningEnabled}
            onChange={setCloudLearningContext}
            label="云端参考学习记录"
          />
        </Field>
        <Field label="保留天数" hint="范围 1～3650 天，过期记录会自动清理。">
          <NumberInput value={prefs.retentionDays} min={1} max={3650} onValueChange={(retentionDays) => void save({ ...prefs, retentionDays })} aria-label="历史保留天数" />
        </Field>
        <Field label="学习记忆保留天数" hint="已提炼的最小化纠错规则独立保留，范围 1～3650 天。">
          <NumberInput value={prefs.learningMemoryRetentionDays} min={1} max={3650} onValueChange={(learningMemoryRetentionDays) => void save({ ...prefs, learningMemoryRetentionDays })} aria-label="学习记忆保留天数" />
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
        <Field label="使用统计" hint="统计只包含成功次数、字数和时长等聚合数字；清空历史不会影响它。">
          <Button variant="dangerHover" onClick={() => void clearUsage()}>清空使用统计</Button>
        </Field>
        <Field label="学习记忆" hint="清除纠错规则和表达偏好，但保留历史正文。">
          <Button variant="dangerHover" onClick={() => void clearLearning()}>清空学习记忆</Button>
        </Field>
      </FormGrid>
      {message && <p role="status" className="text-xs text-[var(--color-fg-subtle)]">{message}</p>}
    </SettingsSection>
  );
}
