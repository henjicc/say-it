import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PluginManagerPanel } from "./PluginManagerPanel";
import type { PluginSnapshot, PluginSummary } from "@/features/plugins/pluginInstaller";

const { cmd, listeners } = vi.hoisted(() => ({
  cmd: vi.fn(),
  listeners: new Map<string, (value: unknown) => void>(),
}));
vi.mock("@/lib/tauri", async (original) => ({
  ...await original<typeof import("@/lib/tauri")>(),
  cmd,
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

function snapshot(item = plugin()): PluginSnapshot {
  return { apiVersion: 5, plugins: [item], errors: [] };
}

describe("插件常驻内存", () => {
  beforeEach(() => { cmd.mockReset(); listeners.clear(); });
  afterEach(cleanup);

  it("立即保存，等待期间禁用开关，完成后无成功提示", async () => {
    cmd.mockResolvedValueOnce(snapshot());
    let finish: (value: PluginSnapshot) => void = () => {};
    cmd.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    render(<PluginManagerPanel />);
    const toggle = await screen.findByRole("switch", { name: "本地测试常驻内存" });
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);
    await waitFor(() => expect(cmd).toHaveBeenLastCalledWith("set_provider_model_keep_loaded", { pluginId: "official.test", keepLoaded: true }));
    expect(toggle).toBeDisabled();
    const loaded = plugin();
    loaded.modelPack!.keepLoaded = true;
    loaded.modelPack!.residency = { state: "loaded" };
    finish(snapshot(loaded));
    await waitFor(() => expect(toggle).toBeChecked());
    expect(screen.getByText("已常驻内存")).toBeInTheDocument();
    expect(screen.queryByText(/成功|已保存/)).not.toBeInTheDocument();
  });

  it.each(["disabled", "pending"])("%s 时禁止开启", async (state) => {
    const item = plugin();
    if (state === "disabled") item.enabled = false;
    else item.modelPack!.state = "pending";
    cmd.mockResolvedValue(snapshot(item));
    render(<PluginManagerPanel />);
    expect(await screen.findByRole("switch", { name: "本地测试常驻内存" })).toBeDisabled();
  });

  it.each(["disabled", "pending"])("%s 时已开启仍可关闭", async (state) => {
    const item = plugin();
    item.modelPack!.keepLoaded = true;
    if (state === "disabled") item.enabled = false;
    else item.modelPack!.state = "pending";
    const closed = structuredClone(item);
    closed.modelPack!.keepLoaded = false;
    cmd.mockResolvedValueOnce(snapshot(item)).mockResolvedValueOnce(snapshot(closed));
    render(<PluginManagerPanel />);
    const toggle = await screen.findByRole("switch", { name: "本地测试常驻内存" });
    expect(toggle).toBeEnabled();
    fireEvent.click(toggle);
    await waitFor(() => expect(cmd).toHaveBeenLastCalledWith("set_provider_model_keep_loaded", { pluginId: "official.test", keepLoaded: false }));
    await waitFor(() => expect(toggle).not.toBeChecked());
    expect(toggle).toBeDisabled();
  });

  it("OCR 与连接器没有常驻开关", async () => {
    const ocr = plugin({ modelPack: { engine: "ppocr-mnn", state: "ready", totalBytes: 1, readyBytes: 1, downloadable: false, keepLoaded: false } });
    cmd.mockResolvedValue({ apiVersion: 5, errors: [], plugins: [ocr, plugin({ id: "connector", name: "连接器", runtimeKind: "javascript", modelPack: undefined })] });
    render(<PluginManagerPanel />);
    await screen.findByText("本地测试");
    expect(screen.queryByRole("switch", { name: /常驻内存/ })).not.toBeInTheDocument();
  });

  it("保存失败保留关闭状态，错误可见", async () => {
    cmd.mockResolvedValueOnce(snapshot()).mockRejectedValueOnce(new Error("磁盘只读"));
    render(<PluginManagerPanel />);
    const toggle = await screen.findByRole("switch", { name: "本地测试常驻内存" });
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
    render(<PluginManagerPanel />);
    expect(await screen.findByRole("alert")).toHaveTextContent("常驻加载失败：文件 SHA256 不匹配");
  });
});
