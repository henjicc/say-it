import { fetchQuery, type Query } from "@/hooks/useCachedQuery";
import { audioDevicesQuery } from "@/features/audio/devices";
import { shortcutBindingsQuery } from "@/features/hotkeys/catalog";
import { useLocalModels } from "@/features/models/localModels";
import { hardwareQuery } from "@/features/models/localModelNeeds";
import { historyFirstPageQuery, historyLearningQuery } from "@/views/HistoryView";
import { usageSummaryQuery } from "@/views/history/UsageSummaryLine";
import { diagnosticStatusQuery } from "@/views/SettingsAdvancedPanel";
import { setupStatusQuery } from "@/views/SettingsSetupPanel";
import { dataRootQuery, startupSettingsQuery } from "@/views/SettingsStartupPanel";
import { useDeveloperStore } from "@/store/useDeveloperStore";
import { audioRecordingQuery, traceRunsQuery, traceStatsQuery, traceStatusQuery } from "@/views/developer/DeveloperPage";
import { benchModelsQuery } from "@/views/developer/ModelBenchPanel";

const PAGE_QUERIES: Query<unknown>[] = [
  setupStatusQuery,
  startupSettingsQuery,
  dataRootQuery,
  diagnosticStatusQuery,
  shortcutBindingsQuery,
  historyFirstPageQuery,
  historyLearningQuery,
  usageSummaryQuery,
  hardwareQuery,
  audioDevicesQuery,
];

const DEVELOPER_QUERIES: Query<unknown>[] = [
  traceStatsQuery,
  traceStatusQuery,
  traceRunsQuery,
  audioRecordingQuery,
  benchModelsQuery,
];

/**
 * 窗口启动后把各页打开时要读的数据先读一遍，第一次切过去就是完整的；之后每次打开仍会在后台
 * 刷新。这里读失败不提示：页面打开时会再读一次，失败由页面显示。
 * 不少命令是同步的、跑在后端主线程上，所以一个读完再读下一个，不一下子全压上去。
 */
export async function prefetchPageData() {
  const queries = useDeveloperStore.getState().enabled ? [...PAGE_QUERIES, ...DEVELOPER_QUERIES] : PAGE_QUERIES;
  for (const query of queries) await fetchQuery(query).catch(() => undefined);
  await useLocalModels.getState().load().catch(() => undefined);
}
