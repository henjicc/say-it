import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsProviderPanel } from "./SettingsProviderPanel";

const loadProviders = vi.fn();
const updateProviderConfig = vi.fn();
const invoke = vi.fn();
let profiles: Array<Record<string, unknown>> = [];

vi.mock("@/lib/tauri", () => ({
  CMD: { openExternalLink: "open_external_link", runProviderPluginAction: "run_provider_plugin_action" },
  cmd: (...args: unknown[]) => invoke(...args),
}));

vi.mock("@/store/useProviderStore", () => ({
  useProviderStore: (selector: (state: unknown) => unknown) => selector({
    profiles,
    load: loadProviders,
    updateConfig: updateProviderConfig,
  }),
}));

describe("SettingsProviderPanel", () => {
  afterEach(cleanup);

  beforeEach(() => {
    profiles = [];
    invoke.mockReset().mockResolvedValue(undefined);
    loadProviders.mockReset().mockResolvedValue(undefined);
    updateProviderConfig.mockReset().mockImplementation(async (providerId: string) =>
      profiles.find((profile) => profile.id === providerId));
  });

  it("shows only preset ASR credentials and never renders a save button", () => {
    profiles = [
      {
        id: "llm-groq",
        kind: "llm:groq",
        displayName: "Groq",
        authKind: "api-key",
        capabilities: ["asr", "llm"],
        enabled: true,
        configFields: [{ key: "apiKey", label: "API Key", fieldType: "password", secret: true }],
        status: { hasApiKey: false },
        config: { model: "openai/gpt-oss-20b" },
      },
      {
        id: "volcengine",
        kind: "sdk:volcengine",
        displayName: "火山引擎",
        authKind: "api-key",
        capabilities: ["asr"],
        enabled: true,
        configFields: [{ key: "apiKey", label: "APP Key", fieldType: "password", secret: true }],
        status: { hasApiKey: false },
        config: {},
      },
      {
        id: "siliconflow",
        kind: "sdk:siliconflow",
        displayName: "硅基流动",
        authKind: "api-key",
        capabilities: ["asr"],
        enabled: true,
        configFields: [{ key: "apiKey", label: "API Key", fieldType: "password", secret: true }],
        status: { hasApiKey: false },
        config: {},
      },
    ];

    render(<SettingsProviderPanel />);

    expect(screen.getByLabelText("APP Key")).toBeInTheDocument();
    const apiKeyLinks = screen.getAllByRole("link", { name: "点击此处获取 API Key" });
    expect(apiKeyLinks.map((link) => link.getAttribute("href"))).toEqual(expect.arrayContaining([
      "https://console.groq.com/keys",
      "https://cloud.siliconflow.cn/account/ak",
    ]));
    const volcengineLink = screen.getByRole("link", { name: "点击此处获取 APP Key" });
    expect(volcengineLink).toHaveAttribute(
      "href",
      "https://console.volcengine.com/speech/new/setting/apikeys",
    );
    fireEvent.click(volcengineLink);
    expect(invoke).toHaveBeenCalledWith("open_external_link", {
      url: "https://console.volcengine.com/speech/new/setting/apikeys",
    });
    expect(screen.queryByText("模型")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /保存/ })).not.toBeInTheDocument();
  });

  it("auto-saves plugin fields and Bailian advanced settings without success prompts", async () => {
    profiles = [
      {
        id: "bailian",
        kind: "sdk:bailian",
        displayName: "阿里云百炼",
        authKind: "api-key",
        capabilities: ["asr"],
        enabled: true,
        configFields: [{ key: "apiKey", label: "API Key", fieldType: "password", secret: true }],
        status: { hasApiKey: false },
        config: {},
      },
      {
        id: "fixture-asr",
        kind: "plugin:fixture-asr",
        displayName: "Fixture ASR",
        authKind: "api-key",
        capabilities: ["asr"],
        enabled: true,
        configFields: [
          { key: "token", label: "Token", fieldType: "password", secret: true },
          { key: "region", label: "区域", fieldType: "text", secret: false },
        ],
        status: { hasApiKey: false },
        config: { region: "cn" },
      },
    ];

    render(<SettingsProviderPanel />);

    const region = screen.getByLabelText("区域");
    fireEvent.change(region, { target: { value: "global" } });
    fireEvent.blur(region);
    await waitFor(() => expect(updateProviderConfig).toHaveBeenCalledWith("fixture-asr", {
      region: "global",
    }));

    fireEvent.click(screen.getByLabelText(/心跳包/));
    await waitFor(() => expect(updateProviderConfig).toHaveBeenCalledWith("bailian", expect.objectContaining({
      heartbeat: true,
    })));

    expect(screen.queryByText(/已保存|保存成功/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /保存/ })).not.toBeInTheDocument();
  });
});
