import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { PluginManagerPanel } from "./PluginManagerPanel";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@/features/plugins/pluginInstaller", () => ({ refreshPluginConsumers: vi.fn() }));
vi.mock("@/store/usePluginImportStore", () => ({
  usePluginImportStore: (selector: (state: { enqueue: () => void }) => unknown) =>
    selector({ enqueue: vi.fn() }),
}));
vi.mock("@/lib/tauri", () => ({
  CMD: { listProviderPlugins: "list_provider_plugins" },
  EVT: {},
  on: vi.fn(async () => () => undefined),
  cmd: vi.fn(async () => ({
    apiVersion: 5,
    errors: [],
    plugins: [{
      id: "official.ppocr-v6-tiny",
      name: "PP-OCRv6 Tiny 本地模型",
      version: "1.0.0",
      providerId: "local-ppocr",
      permissions: [],
      models: ["local-ppocr-v6-tiny"],
      trust: "unsigned",
      actions: [],
      hasBrowserSession: false,
      enabled: false,
      runtimeKind: "model-pack",
      modelPack: {
        engine: "ppocr-mnn", state: "superseded",
        totalBytes: 3180668, readyBytes: 0, downloadable: false,
      },
    }],
  })),
}));

afterEach(cleanup);

it("被内置取代的旧包只提供卸载，不显示启用开关或下载操作", async () => {
  render(<PluginManagerPanel />);
  expect(await screen.findByText("已内置，可卸载")).toBeInTheDocument();
  expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "下载模型" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "卸载PP-OCRv6 Tiny 本地模型" })).toBeInTheDocument();
});
