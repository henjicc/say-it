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

/** 写入是排在微任务队列上的，断言前先让出一轮。 */
const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("热词写入的串行与新鲜度", () => {
  /// 增删改都按下标计算。原实现用渲染时捕获的数组算负载再 `void patch(...)`：
  /// 第一次保存还没返回就做第二次操作，后者基于旧数组算出的负载会把前者整份覆盖掉。
  /// 用户连着删两条，结果只删掉一条——而且没有任何提示。
  it("保存往返期间的第二次操作基于最新状态计算，不覆盖前一次", async () => {
    const pending: Array<() => void> = [];
    cmd.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          pending.push(() => resolve());
        }),
    );
    hydrateCustomizationPrefs({
      hotwords: [
        { text: "甲", weight: 4 },
        { text: "乙", weight: 4 },
        { text: "丙", weight: 4 },
      ],
      contextTemplate: "",
    });
    const { patch } = useCustomizationStore.getState();

    // 连着删两条：第一次删「甲」，第二次删剩下里的第一条（也就是「乙」）。
    const first = patch((current) => ({ hotwords: current.hotwords.filter((_, i) => i !== 0) }));
    const second = patch((current) => ({ hotwords: current.hotwords.filter((_, i) => i !== 0) }));

    // 串行：第二次必须等第一次落盘之后才发出。
    await tick();
    expect(cmd).toHaveBeenCalledTimes(1);
    pending.shift()!();
    await first;
    await tick();
    expect(cmd).toHaveBeenCalledTimes(2);
    pending.shift()!();
    await second;

    expect(useCustomizationStore.getState().prefs.hotwords.map((item) => item.text)).toEqual([
      "丙",
    ]);
  });

  /// 后端校验（单条 64 字符、模板 4000 字符）拒绝时，浮动的 promise 把错误吞掉，
  /// store 不更新，受控输入框无声回滚。调用方必须拿得到这个 rejection。
  it("后端拒绝时 patch 会 reject，且本地状态不被改动", async () => {
    hydrateCustomizationPrefs({ hotwords: [{ text: "原词", weight: 4 }], contextTemplate: "" });
    cmd.mockRejectedValueOnce(new Error("单条热词不能超过 64 个字符"));

    await expect(
      useCustomizationStore.getState().patch({ hotwords: [{ text: "x".repeat(80), weight: 4 }] }),
    ).rejects.toThrow(/64/);

    expect(useCustomizationStore.getState().prefs.hotwords[0].text).toBe("原词");
  });

  it("一次失败不会堵死后续写入", async () => {
    hydrateCustomizationPrefs({ hotwords: [], contextTemplate: "" });
    cmd.mockRejectedValueOnce(new Error("磁盘满了"));

    await expect(
      useCustomizationStore.getState().patch({ contextTemplate: "第一次" }),
    ).rejects.toThrow(/磁盘满了/);

    cmd.mockResolvedValue({ providers: [], results: [] });
    await useCustomizationStore.getState().patch({ contextTemplate: "第二次" });
    expect(useCustomizationStore.getState().prefs.contextTemplate).toBe("第二次");
  });
});
