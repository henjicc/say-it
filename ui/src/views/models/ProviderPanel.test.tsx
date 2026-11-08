import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderProfile } from "@/store/useProviderStore";
import { llmModelPickerOptions } from "@/features/models/llmModelOptions";
import { ProviderPanel } from "./ProviderPanel";
import { AddProviderModal } from "./AddProviderModal";

const invoke = vi.fn();
const updateConfig = vi.fn();
const refreshLlmModels = vi.fn();
const addLlmProvider = vi.fn();
let profiles: ProviderProfile[] = [];

vi.mock("@/lib/tauri", () => ({
  CMD: { openExternalLink: "open_external_link", runProviderPluginAction: "run_provider_plugin_action" },
  cmd: (...args: unknown[]) => invoke(...args),
}));

vi.mock("@/features/asr/modelRegistry", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/features/asr/modelRegistry")>(),
  useModelCatalogRevision: () => 0,
  currentCatalog: () => ({
    models: [
      { id: "fun-asr-realtime", label: "Fun-ASR 实时", providerId: "bailian", category: "realtime", protocol: "dashscope-duplex", scenes: [] },
    ],
  }),
}));

vi.mock("@/store/useProviderStore", () => {
  const state = () => ({
    profiles,
    defaults: { asr: "bailian", llm: "", translation: "", ocr: "" },
    load: vi.fn(),
    updateConfig,
    refreshLlmModels,
    addLlmProvider,
    removeLlmProvider: vi.fn(),
  });
  const useProviderStore = (selector: (value: ReturnType<typeof state>) => unknown) => selector(state());
  useProviderStore.getState = state;
  return { useProviderStore };
});

const llmModel = (name: string, source: "remote" | "manual") => ({
  name, source, availability: "available", reasoningEffort: "auto", temperature: 0.1, maxTokens: null,
});

function bailian(config: Record<string, unknown> = {}, hasApiKey = true): ProviderProfile {
  return {
    id: "bailian", kind: "sdk:bailian", displayName: "阿里云百炼", authKind: "api-key",
    capabilities: ["asr", "llm", "translation"], enabled: true,
    configFields: [{ key: "apiKey", label: "API Key", fieldType: "password", secret: true }],
    actions: ["manageHotwords", "testRealtimeAsr"],
    status: { hasApiKey }, config: { modelListAttemptedAt: 1, ...config },
  };
}

describe("ProviderPanel", () => {
  afterEach(cleanup);
  beforeEach(() => {
    invoke.mockReset().mockResolvedValue(undefined);
    refreshLlmModels.mockReset().mockResolvedValue(undefined);
    updateConfig.mockReset().mockImplementation(async (id: string) => profiles.find((profile) => profile.id === id));
  });

  it("一处展示密钥、识别、翻译与大模型，开关写入显示设置", async () => {
    profiles = [bailian({ model: "qwen3.8-max", models: [llmModel("qwen3.8-max", "remote"), llmModel("qwen3.8-flash", "remote"), llmModel("qwen-long", "remote")] })];
    render(<ProviderPanel profile={profiles[0]} />);

    expect(screen.getByLabelText("API Key")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /获取 API Key/ })).toHaveAttribute("href", expect.stringContaining("bailian.console.aliyun.com"));
    expect(screen.getByRole("switch", { name: "在用途中显示 Qwen3.8 Max" })).toHaveAttribute("aria-checked", "true");
    // 推荐模型即使来自远端列表也默认显示，并用规范的显示名；其余远端模型默认隐藏。
    expect(screen.getByRole("switch", { name: "在用途中显示 Qwen3.8 Flash" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByText("Qwen3.8 Flash")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "在用途中显示 qwen-long", hidden: true })).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText("未显示的模型（1）")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("switch", { name: "在用途中显示 Fun-ASR 实时（实时）" }));
    await waitFor(() => expect(updateConfig).toHaveBeenCalledWith("bailian", {
      modelVisibility: { "fun-asr-realtime": false },
    }));
    fireEvent.click(screen.getByRole("switch", { name: /在用途中显示 Qwen-MT-Flash/ }));
    await waitFor(() => expect(updateConfig).toHaveBeenCalledWith("bailian", {
      modelVisibility: { "qwen-mt-flash": false },
    }));
  });

  it("没填密钥时模型开关不可用，填写后自动保存且不出现保存按钮", async () => {
    profiles = [bailian({}, false)];
    render(<ProviderPanel profile={profiles[0]} />);

    expect(screen.getByRole("switch", { name: "在用途中显示 Fun-ASR 实时（实时）" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("API Key"), { target: { value: "sk-test" } });
    fireEvent.blur(screen.getByLabelText("API Key"));
    await waitFor(() => expect(updateConfig).toHaveBeenCalledWith("bailian", { apiKey: "sk-test" }));
    expect(screen.queryByRole("button", { name: /保存/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /testRealtimeAsr/ })).not.toBeInTheDocument();
  });

  it("插件字段与百炼高级参数自动保存", async () => {
    profiles = [{
      id: "fixture-asr", kind: "plugin:fixture-asr", displayName: "Fixture ASR", authKind: "api-key",
      capabilities: ["asr"], enabled: true,
      configFields: [
        { key: "token", label: "Token", fieldType: "password", secret: true },
        { key: "region", label: "区域", fieldType: "text", secret: false },
      ],
      status: { hasApiKey: false }, config: { region: "cn" },
    }, bailian()];
    const { unmount } = render(<ProviderPanel profile={profiles[0]} />);
    fireEvent.change(screen.getByLabelText("区域"), { target: { value: "global" } });
    fireEvent.blur(screen.getByLabelText("区域"));
    await waitFor(() => expect(updateConfig).toHaveBeenCalledWith("fixture-asr", { region: "global" }));
    unmount();

    render(<ProviderPanel profile={profiles[1]} />);
    fireEvent.click(screen.getByRole("button", { name: /识别高级设置/ }));
    fireEvent.click(screen.getByLabelText(/心跳包/));
    await waitFor(() => expect(updateConfig).toHaveBeenCalledWith("bailian", expect.objectContaining({ heartbeat: true })));
  });
});

describe("llmModelPickerOptions", () => {
  it("只列已配置且设为显示的大模型，当前选中的始终保留", () => {
    const profile = bailian({
      model: "qwen3.8-max",
      models: [llmModel("qwen3.8-max", "remote"), llmModel("qwen3.8-flash", "remote"), llmModel("my-model", "manual"), llmModel("qwen3.7-plus", "remote")],
      modelVisibility: { "qwen3.7-plus": true, "my-model": false },
    });
    const selected = JSON.stringify(["bailian", "qwen3.8-flash"]);
    expect(llmModelPickerOptions([profile], selected).map((option) => option.label)).toEqual(["Qwen3.7 Plus", "Qwen3.8 Flash", "Qwen3.8 Max"]);
    expect(llmModelPickerOptions([{ ...profile, status: { hasApiKey: false } }])).toEqual([]);
  });
});

describe("AddProviderModal", () => {
  afterEach(cleanup);

  it("只列未内置的大模型服务，填写 API Key 后才能添加", async () => {
    addLlmProvider.mockReset().mockResolvedValue({ id: "llm-deepseek" });
    const onAdded = vi.fn();
    render(<AddProviderModal open onClose={vi.fn()} onAdded={onAdded} />);
    const dialog = screen.getByRole("dialog", { name: "添加大模型服务" });

    fireEvent.click(within(dialog).getByRole("combobox"));
    expect(screen.queryByRole("option", { name: /百炼|Groq/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "DeepSeek" }));

    const add = within(dialog).getByRole("button", { name: "添加" });
    expect(add).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("API Key"), { target: { value: "sk-test" } });
    fireEvent.click(add);
    await waitFor(() => expect(addLlmProvider).toHaveBeenCalledWith(expect.objectContaining({ adapter: "deepseek", apiKey: "sk-test" })));
    expect(onAdded).toHaveBeenCalledWith("llm-deepseek");
  });
});
