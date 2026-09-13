import { beforeEach, describe, expect, it, vi } from "vitest";

const cmd = vi.fn();
vi.mock("@/lib/tauri", () => ({
  CMD: { updateAppSettings: "update_app_settings" },
  cmd: (...args: unknown[]) => cmd(...args),
}));
vi.mock("@/store/useProviderStore", () => ({
  useProviderStore: { getState: () => ({ profiles: [], hydrateCatalog: vi.fn() }) },
}));

const { useCustomizationStore, hydrateCustomizationPrefs } = await import("./useCustomizationStore");

const initial = useCustomizationStore.getState();

beforeEach(() => {
  cmd.mockReset();
  cmd.mockResolvedValue({ providers: [], results: [] });
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
