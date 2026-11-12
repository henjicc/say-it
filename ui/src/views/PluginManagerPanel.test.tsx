import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LocalModelsPanel, PluginsPanel } from "./PluginManagerPanel";
import type { PluginSnapshot, PluginSummary } from "@/features/plugins/pluginInstaller";
import { useLocalModels } from "@/features/models/localModels";

const { cmd, listeners, officialPacks } = vi.hoisted(() => ({
  cmd: vi.fn(),
  listeners: new Map<string, (value: unknown) => void>(),
  officialPacks: [] as unknown[],
}));
vi.mock("@/lib/tauri", async (original) => ({
  ...await original<typeof import("@/lib/tauri")>(),
  // 官方模型目录和硬件信息单独应答，不占用各用例为插件列表排好的返回顺序。
  cmd: (name: string, args?: unknown) => name === "list_official_model_packs"
    ? Promise.resolve(officialPacks)
    : name === "detect_hardware" ? Promise.resolve(undefined) : cmd(name, args),
  on: vi.fn(async (event: string, listener: (value: unknown) => void) => {
    listeners.set(event, listener);
    return () => listeners.delete(event);
  }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@/features/plugins/pluginInstaller", () => ({ refreshPluginConsumers: vi.fn() }));

function plugin(overrides: Partial<PluginSummary> = {}): PluginSummary {
  return {
    id: "official.test", name: "本地测试", version: "1", providerId: "local-test", permissions: [],
    models: [], trust: "trusted", actions: [], hasBrowserSession: false, enabled: true, runtimeKind: "model-pack",
    modelPack: { engine: "sherpa-onnx-offline", state: "ready", totalBytes: 100, readyBytes: 100,
      downloadable: true, keepLoaded: false, residency: { state: "unloaded" } },
    ...overrides,
  };
}

// 下载状态放在组件之外，各用例之间要清掉。
function resetDownloads() {
  useLocalModels.setState({ snapshot: undefined, packs: undefined, downloaded: {}, installing: [], installError: undefined, loadError: "", packsError: "" });
}

function snapshot(item = plugin()): PluginSnapshot {
  return { apiVersion: 5, plugins: [item], errors: [] };
}

describe("插件常驻内存", () => {
  beforeEach(() => { cmd.mockReset(); listeners.clear(); resetDownloads(); });
  afterEach(cleanup);

  it("立即保存，等待期间禁用开关，完成后无成功提示", async () => {
    cmd.mockResolvedValueOnce(snapshot());
    let finish: (value: PluginSnapshot) => void = () => {};
    cmd.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    render(<LocalModelsPanel />);
    const toggle = await screen.findByRole("checkbox", { name: "常驻内存" });
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);
    await waitFor(() => expect(cmd).toHaveBeenLastCalledWith("set_provider_model_keep_loaded", { pluginId: "official.test", keepLoaded: true }));
    expect(toggle).toBeDisabled();
    const loaded = plugin();
    loaded.modelPack!.keepLoaded = true;
    loaded.modelPack!.residency = { state: "loaded" };
    finish(snapshot(loaded));
    await waitFor(() => expect(toggle).toBeChecked());
    expect(screen.queryByText(/成功|已保存/)).not.toBeInTheDocument();
  });

  it.each(["disabled", "pending"])("%s 时禁止开启", async (state) => {
    const item = plugin();
    if (state === "disabled") item.enabled = false;
    else item.modelPack!.state = "pending";
    cmd.mockResolvedValue(snapshot(item));
    render(<LocalModelsPanel />);
    expect(await screen.findByRole("checkbox", { name: "常驻内存" })).toBeDisabled();
  });

  it.each(["disabled", "pending"])("%s 时已开启仍可关闭", async (state) => {
    const item = plugin();
    item.modelPack!.keepLoaded = true;
    if (state === "disabled") item.enabled = false;
    else item.modelPack!.state = "pending";
    const closed = structuredClone(item);
    closed.modelPack!.keepLoaded = false;
    cmd.mockResolvedValueOnce(snapshot(item)).mockResolvedValueOnce(snapshot(closed));
    render(<LocalModelsPanel />);
    const toggle = await screen.findByRole("checkbox", { name: "常驻内存" });
    expect(toggle).toBeEnabled();
    fireEvent.click(toggle);
    await waitFor(() => expect(cmd).toHaveBeenLastCalledWith("set_provider_model_keep_loaded", { pluginId: "official.test", keepLoaded: false }));
    await waitFor(() => expect(toggle).not.toBeChecked());
    expect(toggle).toBeDisabled();
  });

  it("OCR 模型没有常驻开关", async () => {
    const ocr = plugin({ modelPack: { engine: "ppocr-mnn", state: "ready", totalBytes: 1, readyBytes: 1, downloadable: false, keepLoaded: false } });
    cmd.mockResolvedValue(snapshot(ocr));
    render(<LocalModelsPanel />);
    await screen.findByText("本地测试");
    expect(screen.queryByRole("checkbox", { name: /常驻内存/ })).not.toBeInTheDocument();
  });

  it("保存失败保留关闭状态，错误可见", async () => {
    cmd.mockResolvedValueOnce(snapshot()).mockRejectedValueOnce(new Error("磁盘只读"));
    render(<LocalModelsPanel />);
    const toggle = await screen.findByRole("checkbox", { name: "常驻内存" });
    fireEvent.click(toggle);
    expect(await screen.findByText(/更新常驻内存失败：.*磁盘只读/)).toBeInTheDocument();
    expect(toggle).not.toBeChecked();
    expect(toggle).toBeEnabled();
  });

  it("加载失败原因持续可见", async () => {
    const item = plugin();
    item.modelPack!.keepLoaded = true;
    item.modelPack!.residency = { state: "failed", error: "文件 SHA256 不匹配" };
    cmd.mockResolvedValue(snapshot(item));
    render(<LocalModelsPanel />);
    expect(await screen.findByRole("alert")).toHaveTextContent("常驻加载失败：文件 SHA256 不匹配");
  });
});

describe("被内置取代的模型包", () => {
  beforeEach(() => { cmd.mockReset(); listeners.clear(); resetDownloads(); });
  afterEach(cleanup);

  it("只提供卸载，不显示启用开关或下载操作", async () => {
    cmd.mockResolvedValue(snapshot(plugin({
      id: "official.ppocr-v6-tiny", name: "PP-OCRv6 Tiny 本地模型", version: "1.0.0", providerId: "local-ppocr",
      models: ["local-ppocr-v6-tiny"], trust: "unsigned", enabled: false,
      modelPack: { engine: "ppocr-mnn", state: "superseded", totalBytes: 3180668, readyBytes: 0, downloadable: false, keepLoaded: false },
    })));
    render(<LocalModelsPanel />);
    expect(await screen.findByText("已内置，可卸载")).toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "下载模型" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "卸载PP-OCRv6 Tiny 本地模型" })).toBeInTheDocument();
  });
});

describe("官方模型", () => {
  beforeEach(() => { cmd.mockReset(); listeners.clear(); resetDownloads(); });
  afterEach(() => { cleanup(); officialPacks.length = 0; });

  it("已装的留在目录原位且不再给下载，未装的点下载会安装对应模型包", async () => {
    officialPacks.push(
      { id: "official.test", name: "本地测试", description: "测试", version: "1", sizeBytes: 100 },
      { id: "official.punct", name: "标点", description: "加标点", version: "1.0.0", sizeBytes: 100 },
    );
    cmd.mockResolvedValue(snapshot());
    render(<LocalModelsPanel />);
    fireEvent.click(await screen.findByRole("button", { name: "下载标点" }));
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("install_official_model_pack", { packId: "official.punct" }));
    expect(screen.queryByRole("button", { name: /^(下载|更新|继续下载)本地测试$/ })).toBeNull();
    expect(screen.getByRole("switch", { name: "启用本地测试" })).toBeInTheDocument();
  });

  it("目录版本更高才显示更新", async () => {
    officialPacks.push(
      { id: "official.test", name: "本地测试", description: "测试", version: "1.0.10", sizeBytes: 100 },
      { id: "official.old", name: "旧目录", description: "测试", version: "1.0.0", sizeBytes: 100 },
    );
    cmd.mockResolvedValue({ apiVersion: 5, errors: [], plugins: [plugin({ version: "1.0.9" }), plugin({ id: "official.old", name: "旧目录", version: "1.0.1" })] });
    render(<LocalModelsPanel />);
    expect(await screen.findByRole("button", { name: "更新本地测试" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^(下载|更新|继续下载)旧目录$/ })).toBeNull();
  });

  it("切到别的页面再回来，进行中的下载仍显示进度", async () => {
    officialPacks.push({ id: "official.punct", name: "标点", description: "加标点", version: "1", sizeBytes: 200 });
    cmd.mockResolvedValue(snapshot());
    useLocalModels.setState({ installing: ["official.punct"], downloaded: { "official.punct": 50 } });
    render(<LocalModelsPanel />);
    expect(await screen.findByRole("progressbar", { name: "标点下载进度" })).toHaveAttribute("aria-valuenow", "25");
    expect(screen.queryByRole("button", { name: "下载标点" })).toBeNull();
  });

  it("清单已装但权重没下完时继续下载，不重装清单", async () => {
    officialPacks.push({ id: "official.test", name: "本地测试", description: "测试", version: "1", sizeBytes: 100 });
    const partial = plugin();
    partial.modelPack!.state = "partial";
    cmd.mockResolvedValue(snapshot(partial));
    render(<LocalModelsPanel />);
    fireEvent.click(await screen.findByRole("button", { name: "继续下载本地测试" }));
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("download_provider_model_pack", { pluginId: "official.test" }));
    expect(cmd).not.toHaveBeenCalledWith("install_official_model_pack", expect.anything());
  });
});

describe("插件页", () => {
  beforeEach(() => { cmd.mockReset(); listeners.clear(); resetDownloads(); });
  afterEach(cleanup);

  it("只列连接器插件，模型包归本地模型页", async () => {
    cmd.mockResolvedValue({ apiVersion: 5, errors: [], plugins: [plugin(), plugin({ id: "connector", name: "连接器", runtimeKind: "javascript", modelPack: undefined })] });
    render(<PluginsPanel />);
    expect(await screen.findByText("连接器")).toBeInTheDocument();
    expect(screen.queryByText("本地测试")).not.toBeInTheDocument();
  });
});
