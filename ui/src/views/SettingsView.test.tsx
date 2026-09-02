import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useUiStore } from "@/store/useUiStore";

vi.mock("@/views/SettingsProviderPanel", () => ({ SettingsProviderPanel: () => <section><h2>ASR 供应商</h2><button>配置识别模型</button></section> }));
vi.mock("@/views/SettingsLlmPanel", () => ({ SettingsLlmPanel: () => <section><h2>大语言模型</h2><button>配置语言模型</button></section> }));
vi.mock("@/views/PluginManagerPanel", () => ({ PluginManagerPanel: () => <section><h2>插件管理</h2><button>安装插件</button></section> }));
vi.mock("@/views/SettingsMicCuePanel", () => ({ SettingsMicCuePanel: () => <section><h2>提示音</h2><button>启用音频提示</button></section> }));
vi.mock("@/views/SettingsStartupPanel", () => ({ SettingsStartupPanel: () => <section><h2>启动设置</h2><div><p>开机自启</p><button>切换开机自启</button></div></section> }));
vi.mock("@/views/SettingsSetupPanel", () => ({ SettingsSetupPanel: () => <section><h2>使用引导</h2><button>重新运行</button></section> }));
vi.mock("@/views/SettingsAppearancePanel", () => ({ SettingsAppearancePanel: () => <section><h2>外观</h2><label><span>强调色</span><input aria-label="强调色值" /></label></section> }));
vi.mock("@/views/SettingsHistoryPanel", () => ({ SettingsHistoryPanel: () => <section><h2>本地历史</h2><button>保存历史</button></section> }));
vi.mock("@/views/SettingsKeyBindingsPanel", () => ({ SettingsKeyBindingsPanel: () => <section><h2>集中管理</h2><button>修改快捷键</button></section> }));
vi.mock("@/views/SettingsComparePanel", () => ({ SettingsComparePanel: () => <section><button>开始对比</button></section> }));
vi.mock("@/views/SettingsAdvancedPanel", () => ({ SettingsAdvancedPanel: () => <section><h2>诊断日志</h2><button>打开日志目录</button></section> }));

import { filterSettings } from "@/features/settings/SettingsSearch";
import { SettingsView } from "./SettingsView";

describe("SettingsView", () => {
  beforeEach(() => {
    useUiStore.setState({ settingsTab: "general" });
    HTMLElement.prototype.scrollIntoView = vi.fn();
  });

  afterEach(cleanup);

  it("replaces the descriptive copy with searchable setting results", () => {
    render(<SettingsView />);

    expect(screen.queryByText(/配置识别模型与密钥/)).not.toBeInTheDocument();
    const search = screen.getByRole("combobox", { name: "搜索设置项" });
    fireEvent.change(search, { target: { value: "提示音" } });

    expect(screen.getByRole("option", { name: /提示音/ })).toBeInTheDocument();
  });

  it("switches tabs and focuses the matched control", async () => {
    render(<SettingsView />);

    const search = screen.getByRole("combobox", { name: "搜索设置项" });
    fireEvent.change(search, { target: { value: "诊断" } });
    fireEvent.click(screen.getByRole("option", { name: /诊断日志/ }));

    await waitFor(() => expect(useUiStore.getState().settingsTab).toBe("advanced"));
    await waitFor(() => expect(screen.getByRole("button", { name: "打开日志目录" })).toHaveFocus());
  });

  it("matches aliases as well as visible labels", () => {
    expect(filterSettings("api key").map((item) => item.id)).toEqual(expect.arrayContaining([
      "asr-providers",
      "ocr-providers",
      "translation-providers",
      "llm-providers",
    ]));
    expect(filterSettings("开机启动")[0]?.id).toBe("autostart");
  });
});
