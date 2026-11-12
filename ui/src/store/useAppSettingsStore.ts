import { create } from "zustand";
import { CMD, cmd, type AppSettings } from "@/lib/tauri";

/** 设置领域与快照里对应字段；诊断按字段合并保存，其余整份替换。 */
const DOMAIN_FIELDS = {
  assistant: "assistantPrefs",
  history: "historyPrefs",
  diagnostics: "diagnosticsPrefs",
} as const satisfies Record<string, keyof AppSettings>;

type SettingsDomain = keyof typeof DOMAIN_FIELDS;
type DomainSettings = Pick<AppSettings, (typeof DOMAIN_FIELDS)[SettingsDomain]>;

/**
 * 还没有独立 store 的设置领域（助手、历史、诊断）：启动时写入，每次保存用后端返回的设置刷新。
 * 这几页从这里读，打开时不必再读一遍快照。其他领域各有自己的 store，不放在这里。
 */
export const useAppSettings = create<{ settings?: DomainSettings }>(() => ({}));

export function hydrateAppSettings(settings: AppSettings) {
  useAppSettings.setState({
    settings: {
      assistantPrefs: settings.assistantPrefs,
      historyPrefs: settings.historyPrefs,
      diagnosticsPrefs: settings.diagnosticsPrefs,
    },
  });
}

/**
 * 先在界面上生效，再保存；失败时把这个领域退回保存前的值（期间别处又改过就不动）并抛出错误。
 * 启动时没能载入设置就拒绝保存，免得用默认值整份覆盖磁盘上的真实配置。
 */
export async function saveAppSettings(domain: SettingsDomain, value: Record<string, unknown>) {
  const current = useAppSettings.getState().settings;
  if (!current) throw new Error("设置还没有载入，暂时不能保存，请重启应用");
  const field = DOMAIN_FIELDS[domain];
  const previous = current[field];
  const optimistic = domain === "diagnostics" ? { ...previous, ...value } : value;
  useAppSettings.setState({ settings: { ...current, [field]: optimistic } });
  try {
    hydrateAppSettings(await cmd<AppSettings>(CMD.updateAppSettings, { domain, value }));
  } catch (error) {
    const latest = useAppSettings.getState().settings;
    if (latest && latest[field] === optimistic) {
      useAppSettings.setState({ settings: { ...latest, [field]: previous } });
    }
    throw error;
  }
}
