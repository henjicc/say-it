import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const cmd = vi.fn();
vi.mock("@/lib/tauri", () => ({
  CMD: {
    updateAppSettings: "update_app_settings",
    customizationSyncProviders: "customization_sync_providers",
    customizationPullFromProvider: "customization_pull_from_provider",
    customizationClearProviders: "customization_clear_providers",
  },
  cmd: (...args: unknown[]) => cmd(...args),
}));
let profiles: unknown[] = [];
const hydrateCatalog = vi.fn();
vi.mock("@/store/useProviderStore", () => ({
  useProviderStore: { getState: () => ({ profiles, hydrateCatalog }) },
}));

const { useCustomizationStore, hydrateCustomizationPrefs } = await import("./useCustomizationStore");

const initial = useCustomizationStore.getState();

beforeEach(() => {
  cmd.mockReset();
  hydrateCatalog.mockReset();
  cmd.mockResolvedValue({ providers: [], results: [] });
  profiles = [];
  useCustomizationStore.setState({ ...initial, hydrated: false });
});

describe("useCustomizationStore 的写入闸口", () => {
  /// 本 store 没有 localStorage 镜像，热词与上下文模板的唯一来源就是启动时的
  /// hydrate。初始化失败时 store 停在空默认值上，此时若允许写入，用户随手加一条
  /// 热词就会把磁盘上真实的几百条热词与上下文模板整份覆盖掉，且不可恢复。
  it("未从后端填充时拒绝写入，绝不用默认值覆盖磁盘上的真实配置", async () => {
    await expect(
      useCustomizationStore.getState().patch({ hotwords: [{ text: "新词", weight: 4 }] }),
    ).rejects.toThrow(
      /尚未加载完成/,
    );
    expect(cmd).not.toHaveBeenCalled();
  });

  it("填充之后才允许写入，并把合并结果发给后端", async () => {
    hydrateCustomizationPrefs({
      hotwords: [{ text: "原有词", weight: 4 }],
      contextTemplate: "原模板",
    });
    expect(useCustomizationStore.getState().hydrated).toBe(true);

    await useCustomizationStore.getState().patch({ contextTemplate: "新模板" });

    expect(cmd).toHaveBeenCalledWith("update_app_settings", {
      domain: "customization",
      value: expect.objectContaining({
        hotwords: [{ text: "原有词", weight: 4 }],
        contextTemplate: "新模板",
      }),
    });
  });
});

describe("热词的云端同步", () => {
  const target = {
    id: "bailian",
    enabled: true,
    capabilities: ["customization"],
    actions: [],
  };

  beforeEach(() => {
    vi.useFakeTimers();
    profiles = [target];
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  /// 删光热词此前会被当成「没东西可同步」提前 return（后端也拒收空热词），于是供应商
  /// 侧的旧词表和 vocabularyIds 一直留着并在识别时继续下发——用户以为删干净了，实际
  /// 毫无变化，且除了手动点「清除云端词表」没有别的纠正入口。
  it("把热词删光时仍然发起同步，用来清除云端词表", async () => {
    hydrateCustomizationPrefs({ hotwords: [{ text: "原有词", weight: 4 }], contextTemplate: "" });
    await useCustomizationStore.getState().patch({ hotwords: [] });

    await vi.advanceTimersByTimeAsync(2_000);

    expect(cmd).toHaveBeenCalledWith("customization_sync_providers");
    expect(useCustomizationStore.getState().syncMessage).toBe("已清除供应商词表");
  });

  it("没有同步目标时不发请求", async () => {
    profiles = [];
    hydrateCustomizationPrefs({ hotwords: [{ text: "原有词", weight: 4 }], contextTemplate: "" });
    await useCustomizationStore.getState().patch({ hotwords: [] });

    await vi.advanceTimersByTimeAsync(2_000);

    expect(cmd).not.toHaveBeenCalledWith("customization_sync_providers");
  });

  it("新增热词照常推送", async () => {
    hydrateCustomizationPrefs({ hotwords: [], contextTemplate: "" });
    await useCustomizationStore.getState().patch({ hotwords: [{ text: "新词", weight: 4 }] });

    await vi.advanceTimersByTimeAsync(2_000);

    expect(cmd).toHaveBeenCalledWith("customization_sync_providers");
    expect(useCustomizationStore.getState().syncMessage).toBe("已同步到供应商");
  });
});
