import { HelpLabel } from "@/components/ui/Tooltip";
import { useMemo, useState } from "react";
import { notify, notifyError } from "@/store/useToastStore";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Group, Row } from "@/components/ui/Group";
import { NumberInput, Textarea } from "@/components/ui/Input";
import { useConfirm } from "@/components/ui/useConfirm";
import { Switch } from "@/components/ui/Switch";
import { CMD, cmd } from "@/lib/tauri";
import { saveAppSettings, useAppSettings } from "@/store/useAppSettingsStore";

interface HistoryPrefs {
  enabled: boolean;
  finalDraftObservationEnabled: boolean;
  correctionLearningEnabled: boolean;
  cloudLearningContextEnabled: boolean;
  learningMemoryRetentionDays: number;
  retentionDays: number;
  excludedApps: string[];
  smartInputRecordingEnabled: boolean;
}

function normalize(value: Record<string, unknown>): HistoryPrefs {
  return {
    enabled: value.enabled !== false,
    finalDraftObservationEnabled: value.finalDraftObservationEnabled === true || value.finalDraftLearningEnabled === true,
    correctionLearningEnabled: value.correctionLearningEnabled === true || value.finalDraftLearningEnabled === true,
    cloudLearningContextEnabled: value.cloudLearningContextEnabled === true,
    learningMemoryRetentionDays: Math.min(3650, Math.max(1, Number(value.learningMemoryRetentionDays) || 180)),
    retentionDays: Math.min(3650, Math.max(1, Number(value.retentionDays) || 30)),
    excludedApps: Array.isArray(value.excludedApps) ? value.excludedApps.filter((item): item is string => typeof item === "string") : [],
    smartInputRecordingEnabled: value.smartInputRecordingEnabled === true,
  };
}

/** 历史偏好：启动时载入的那一份；设置页与简单模式共用。 */
export function useHistoryPrefs() {
  const raw = useAppSettings((state) => state.settings?.historyPrefs);
  const prefs = useMemo(() => normalize(raw ?? {}), [raw]);

  async function save(next: HistoryPrefs) {
    try {
      await saveAppSettings("history", next as unknown as Record<string, unknown>);
    } catch (error) {
      notifyError(error);
    }
  }

  return { prefs, save };
}

export function SettingsHistoryPanel() {
  const { confirm, dialog } = useConfirm();
  const { prefs, save } = useHistoryPrefs();
  const [excludedText, setExcludedText] = useState(() => prefs.excludedApps.join("\n"));

  async function clear() {
    if (
      !(await confirm({
        title: "确认清空全部历史",
        message: "将清空全部本地历史和纠错样本。此操作不可撤销。",
        confirmLabel: "清空历史",
      }))
    ) {
      return;
    }
    try {
      await cmd(CMD.clearHistory);
      notify("历史已清空");
    } catch (error) {
      notifyError(error);
    }
  }

  async function clearUsage() {
    if (
      !(await confirm({
        title: "确认清空使用统计",
        message: "将清空本地累计使用统计，历史记录不会受到影响。",
        confirmLabel: "清空统计",
      }))
    ) {
      return;
    }
    try {
      await cmd(CMD.clearUsageSummary);
      notify("使用统计已清空");
    } catch (error) {
      notifyError(error);
    }
  }

  async function clearLearning() {
    if (
      !(await confirm({
        title: "确认清空学习记忆",
        message: "将清空全部个性化学习记忆，历史正文会保留。此操作不可撤销。",
        confirmLabel: "清空记忆",
      }))
    ) {
      return;
    }
    try {
      await cmd(CMD.clearLearningMemory);
      notify("学习记忆已清空");
    } catch (error) {
      notifyError(error);
    }
  }

  async function setCloudLearningContext(enabled: boolean) {
    if (
      enabled &&
      !(await confirm({
        title: "开启云端学习上下文",
        message:
          "开启后，云端智能处理会接收最多三条与当前文本直接相关、已脱敏的局部纠错，以及一条已确认的表达偏好。是否继续？",
        confirmLabel: "开启",
        danger: false,
      }))
    ) {
      return;
    }
    void save({ ...prefs, cloudLearningContextEnabled: enabled });
  }

  return (
    <InspectorBody>
      <Group title="本地历史" description="历史记录只保存在本机，不保存音频，也不会记录密码输入框中的内容。">
        <Row label="保存历史" controlId="history-enabled" hint="关闭后新任务不再写入，已有记录不会自动删除。">
          <Switch id="history-enabled" checked={prefs.enabled} onChange={(enabled) => void save({ ...prefs, enabled })} label="保存历史" />
        </Row>
        <Row label="保存智能处理输入" controlId="history-smart-input" hint="同时保存每次智能处理发给模型的完整内容（含屏幕识别出的文字）和模型原始输出，用于以后训练本地模型。屏幕文字较多时，每条记录会明显变大。">
          <Switch
            id="history-smart-input"
            checked={prefs.smartInputRecordingEnabled}
            disabled={!prefs.enabled}
            onChange={(smartInputRecordingEnabled) => void save({ ...prefs, smartInputRecordingEnabled })}
            label="保存智能处理输入"
          />
        </Row>
        <Row label="保留天数" hint="范围 1～3650 天，过期记录会自动清理。">
          <NumberInput className="w-32" value={prefs.retentionDays} min={1} max={3650} onValueChange={(retentionDays) => void save({ ...prefs, retentionDays })} aria-label="历史保留天数" />
        </Row>
        <div className="flex flex-col gap-2 p-3.5">
          <span className="text-[13px] text-[var(--color-fg)]"><HelpLabel content="每行一个进程名或应用名，不区分大小写。">排除应用</HelpLabel></span>
          <Textarea value={excludedText} onChange={(event) => setExcludedText(event.target.value)} onBlur={() => {
            const excludedApps = excludedText.split(/\r?\n/).map((item) => item.trim()).filter(Boolean);
            void save({ ...prefs, excludedApps });
          }} placeholder="例如：1Password.exe" aria-label="历史排除应用" />
        </div>
      </Group>

      <Group title="个性化学习">
        <Row label="记录发送前修改" controlId="history-final-draft-observation" hint="仅限本次语音输入对应输入框，最长观察 120 秒；不会常驻采集键盘或其他文本。">
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
        </Row>
        <Row label="个性化纠错" controlId="history-correction-learning" hint="同一处纠正重复两次或经你确认后，才会用于以后的自动纠错。">
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
        </Row>
        <Row label="云端参考学习记录" controlId="history-cloud-learning" hint="开启后，会向云端模型提供最多三条已去除敏感信息的纠错片段，帮助它理解你的习惯。默认关闭。">
          <Switch
            id="history-cloud-learning"
            checked={prefs.cloudLearningContextEnabled}
            disabled={!prefs.correctionLearningEnabled}
            onChange={(enabled) => void setCloudLearningContext(enabled)}
            label="云端参考学习记录"
          />
        </Row>
        <Row label="学习记忆保留天数" hint="已学会的纠错习惯单独保存，可保留 1～3650 天。">
          <NumberInput className="w-32" value={prefs.learningMemoryRetentionDays} min={1} max={3650} onValueChange={(learningMemoryRetentionDays) => void save({ ...prefs, learningMemoryRetentionDays })} aria-label="学习记忆保留天数" />
        </Row>
      </Group>

      <Group title="清理数据">
        <Row label="全部历史" message="立即删除全部历史及其纠错样本，此操作不可撤销。">
          <Button size="sm" variant="danger" onClick={() => void clear()}>清空全部历史</Button>
        </Row>
        <Row label="使用统计" hint="统计只包含次数、字数和时长；清空历史不会清除这些统计。">
          <Button size="sm" variant="danger" onClick={() => void clearUsage()}>清空使用统计</Button>
        </Row>
        <Row label="学习记忆" hint="清除纠错规则和表达偏好，但保留历史正文。">
          <Button size="sm" variant="danger" onClick={() => void clearLearning()}>清空学习记忆</Button>
        </Row>
      </Group>
      {dialog}
    </InspectorBody>
  );
}
