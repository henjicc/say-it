import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsLlmPanel } from "./SettingsLlmPanel";

const invoke = vi.fn();
const addLlmProvider = vi.fn();
const refreshLlmModels = vi.fn();
const setDefault = vi.fn();
let profiles: Array<Record<string, unknown>> = [];

vi.mock("@/lib/tauri", () => ({
  CMD: { openExternalLink: "open_external_link" },
  cmd: (...args: unknown[]) => invoke(...args),
}));

vi.mock("@/store/useProviderStore", () => ({
  useProviderStore: (selector: (state: unknown) => unknown) => selector({
    profiles,
    defaults: { llm: "" },
    addLlmProvider,
    refreshLlmModels,
    setDefault,
  }),
}));

describe("SettingsLlmPanel", () => {
  afterEach(cleanup);
  beforeEach(() => {
    profiles = [];
    invoke.mockReset().mockResolvedValue(undefined);
    addLlmProvider.mockReset().mockResolvedValue({ id: "llm-new" });
    refreshLlmModels.mockReset().mockResolvedValue({ config: { models: [] } });
    setDefault.mockReset().mockResolvedValue(undefined);
  });

  it("shows capability-based plugin LLM without manual or removal controls", () => {
    profiles = [{
      id: "fixture-llm",
      kind: "plugin:fixture-llm",
      displayName: "Fixture LLM",
      authKind: "none",
      capabilities: ["llm"],
      enabled: true,
      configFields: [],
      config: {
        model: "chat",
        models: [{
          name: "chat", source: "remote", availability: "available",
          reasoningEffort: "auto", temperature: null, maxTokens: null,
        }],
      },
    }];
    render(<SettingsLlmPanel />);
    expect(screen.getByText("API Key 在应用私有目录中本地加密保存，不调用系统钥匙链。")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "当前模型" })).toHaveTextContent("chat");
    expect(screen.queryByRole("button", { name: "手动添加" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "删除供应商" })).not.toBeInTheDocument();
  });

  it("keeps preset details hidden and requires an API key before adding", async () => {
    render(<SettingsLlmPanel />);
    fireEvent.click(screen.getByRole("button", { name: "+ 添加" }));

    const dialog = screen.getByRole("dialog", { name: "添加大语言模型" });
    expect(within(dialog).queryByText("显示名称")).not.toBeInTheDocument();
    expect(within(dialog).queryByText("初始模型")).not.toBeInTheDocument();

    const addButton = within(dialog).getByRole("button", { name: "添加" });
    expect(addButton).toBeDisabled();

    const keyLink = within(dialog).getByRole("link", { name: /前往 Groq API Key 管理页/ });
    expect(keyLink).toHaveAttribute("href", "https://console.groq.com/keys");
    fireEvent.click(keyLink);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("open_external_link", {
      url: "https://console.groq.com/keys",
    }));

    fireEvent.change(within(dialog).getByLabelText("API Key（必填）"), {
      target: { value: "gsk-test" },
    });
    expect(addButton).toBeEnabled();
    fireEvent.click(addButton);

    await waitFor(() => expect(addLlmProvider).toHaveBeenCalledWith({
      adapter: "groq",
      displayName: "Groq",
      model: "openai/gpt-oss-20b",
      apiKey: "gsk-test",
      endpoint: "",
    }));
  });
});
