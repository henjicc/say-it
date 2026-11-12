import { HelpLabel } from "@/components/ui/Tooltip";
import { useCachedQuery, type Query } from "@/hooks/useCachedQuery";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { CMD, EVT, cmd, type UsageSummary } from "@/lib/tauri";

function formatDuration(ms: number) {
  const minutes = Math.round(ms / 60_000);
  if (minutes < 60) return `${minutes} 分钟`;
  return `${Math.floor(minutes / 60)} 小时 ${minutes % 60} 分`;
}

export const usageSummaryQuery: Query<UsageSummary> = {
  key: "usageSummary",
  load: () => cmd<UsageSummary>(CMD.getUsageSummary),
};

/** 本地累计使用：只统计次数、字数和时长，不保存正文或音频。 */
export function UsageSummaryLine() {
  const { data: usage, refresh } = useCachedQuery(usageSummaryQuery);
  useTauriEvent(EVT.historyChanged, () => void refresh());
  if (!usage) return null;
  const items = [
    `成功 ${usage.successfulActions.toLocaleString()} 次`,
    `输出 ${usage.outputChars.toLocaleString()} 字`,
    `口述 ${formatDuration(usage.spokenDurationMs)}`,
    `估算节省 ${formatDuration(usage.estimatedTimeSavedMs)}`,
  ];
  return (
    <p className="text-xs text-[var(--color-fg-subtle)]">
      <HelpLabel content="只统计次数、字数和时长，不保存正文或音频。节省时间按每分钟输入 40 个汉字或 40 个英文单词估算。">
        {items.join(" · ")}
      </HelpLabel>
    </p>
  );
}
