import { describe, expect, it, vi } from "vitest";
import { installLocalPacks } from "./localModels";

const invoke = vi.fn();
const refreshPluginConsumers = vi.fn();

vi.mock("@/lib/tauri", () => ({
  CMD: {
    listProviderPlugins: "list_provider_plugins",
    downloadProviderModelPack: "download_provider_model_pack",
    installOfficialModelPack: "install_official_model_pack",
  },
  EVT: {},
  cmd: (...args: unknown[]) => invoke(...args),
  on: vi.fn(),
}));
vi.mock("@/features/plugins/pluginInstaller", () => ({ refreshPluginConsumers: () => refreshPluginConsumers() }));

const pack = (id: string, state: string) => ({ id, modelPack: { state } });

describe("installLocalPacks", () => {
  it("installs missing packs and resumes partial ones in order", async () => {
    // 后端每一步都返回完整快照：标点包只装了清单、权重没下完。
    invoke.mockResolvedValue({ plugins: [pack("official.sherpa-ct-punct-zh-en", "partial")] });

    await installLocalPacks();

    expect(invoke.mock.calls.slice(1)).toEqual([
      ["install_official_model_pack", { packId: "official.sherpa-paraformer-online" }],
      ["download_provider_model_pack", { pluginId: "official.sherpa-ct-punct-zh-en" }],
    ]);
    expect(refreshPluginConsumers).toHaveBeenCalledTimes(1);
  });
});
