import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { OnboardingWizard } from "./OnboardingWizard";

const invoke = vi.fn();
const patchDictPrefs = vi.fn();
const updateProviderConfig = vi.fn();
const loadProviders = vi.fn();
const setView = vi.fn();
const setSettingsTab = vi.fn();

const cloudProvider = {
  id: "bailian",
  kind: "sdk:bailian",
  displayName: "阿里云百炼",
  authKind: "api-key",
  capabilities: ["asr"],
  enabled: true,
  status: { configured: false, hasApiKey: false },
};

vi.mock("@/lib/tauri", () => ({
  CMD: {
    getSetupStatus: "get_setup_status",
    startBackendMic: "start_backend_mic",
    releaseBackendMic: "release_backend_mic",
    requestSetupPermissions: "request_setup_permissions",
    openExternalLink: "open_external_link",
    openApiKeyPage: "open_api_key_page",
    completeOnboarding: "complete_onboarding",
  },
  cmd: (...args: unknown[]) => invoke(...args),
}));

vi.mock("@/lib/platform", () => ({ isMacOS: true }));

vi.mock("@/features/asr/modelOptions", () => ({
  DICTATION_ASR_MODEL_OPTIONS: [
    { value: "cloud-model", label: "云端模型" },
    { value: "local-model", label: "离线模型" },
  ],
}));

vi.mock("@/features/asr/modelRegistry", () => ({
  useModelCatalogRevision: () => 0,
  modelInfo: (id: string) => id === "local-model"
    ? { id, label: "离线模型", providerId: "local", protocol: "local-sherpa-offline" }
    : { id, label: "云端模型", providerId: "bailian", protocol: "dashscope-duplex" },
}));

vi.mock("@/store/useDictPrefs", () => ({
  useDictPrefs: (selector: (state: unknown) => unknown) => selector({
    prefs: { asrModel: "cloud-model", micDeviceId: "" },
    patch: patchDictPrefs,
  }),
}));

vi.mock("@/store/useProviderStore", () => ({
  useProviderStore: (selector: (state: unknown) => unknown) => selector({
    profiles: [cloudProvider],
    load: loadProviders,
    updateConfig: updateProviderConfig,
  }),
}));

vi.mock("@/store/useUiStore", () => ({
  useUiStore: (selector: (state: unknown) => unknown) => selector({ setView, setSettingsTab }),
}));

const blockedPermission = {
  id: "permissions",
  status: "blocked",
  title: "文字输入权限",
  message: "需要辅助功能权限",
  action: "permissions",
};

const setupStatus = {
  onboardingVersion: 0,
  requiredVersion: 1,
  complete: false,
  checks: [blockedPermission],
};

describe("OnboardingWizard", () => {
  afterEach(cleanup);
  beforeEach(() => {
    invoke.mockReset();
    patchDictPrefs.mockReset().mockResolvedValue(undefined);
    loadProviders.mockReset().mockResolvedValue(undefined);
    setView.mockReset();
    setSettingsTab.mockReset();
    updateProviderConfig.mockReset().mockImplementation(async () => {
      cloudProvider.status = { configured: true, hasApiKey: true };
      return cloudProvider;
    });
    cloudProvider.status = { configured: false, hasApiKey: false };
    invoke.mockImplementation(async (name: string) => {
      if (name === "get_setup_status") return setupStatus;
      if (name === "start_backend_mic") return { reused: false };
      if (name === "request_setup_permissions") {
        return { ...blockedPermission, status: "ready", message: "文字输入权限已授予", action: null };
      }
      return undefined;
    });
  });

  it.each(["关闭", "Escape", "遮罩", "完成设置", "打开完整设置", "打开插件管理"])("persists acknowledgement before exiting through %s", async (exit) => {
    let save: (() => void) | undefined;
    invoke.mockImplementation(async (name: string) => {
      if (name === "get_setup_status") return setupStatus;
      if (name === "start_backend_mic") return { reused: false };
      if (name === "complete_onboarding") return new Promise<void>((resolve) => { save = resolve; });
    });
    const onClose = vi.fn();
    render(<OnboardingWizard open onClose={onClose} />);
    await screen.findByText("麦克风可以正常使用");
    expect(invoke).not.toHaveBeenCalledWith("complete_onboarding");

    if (exit === "完成设置" || exit === "打开插件管理" || exit === "打开完整设置") {
      fireEvent.click(screen.getByRole("button", { name: "下一步" }));
      if (exit !== "打开完整设置") fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    }
    if (exit === "Escape") fireEvent.keyDown(window, { key: "Escape" });
    else if (exit === "遮罩") fireEvent.click(screen.getByRole("dialog").parentElement!);
    else fireEvent.click(screen.getByRole("button", { name: exit }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("complete_onboarding"));
    expect(onClose).not.toHaveBeenCalled();
    expect(setView).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(invoke.mock.calls.filter(([name]) => name === "complete_onboarding")).toHaveLength(1);
    save!();
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    if (exit === "打开完整设置" || exit === "打开插件管理") {
      expect(setView).toHaveBeenCalledWith("settings");
      expect(setSettingsTab).toHaveBeenCalledWith(exit === "打开完整设置" ? "model" : "plugins");
    }
  });

  it("keeps the wizard open when persistence fails and lets the user retry", async () => {
    const onClose = vi.fn();
    render(<OnboardingWizard open onClose={onClose} />);
    await screen.findByText("麦克风可以正常使用");
    invoke.mockRejectedValueOnce(new Error("磁盘写入失败"));
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(await screen.findByText(/保存引导状态失败.*磁盘写入失败/)).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("keeps onboarding focused on permissions, model setup, and offline installation", async () => {
    render(<OnboardingWizard open onClose={() => undefined} />);

    expect(screen.getByRole("heading", { name: "授予必要权限" })).toBeInTheDocument();
    expect(await screen.findByText("麦克风可以正常使用")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "授予权限" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("request_setup_permissions"));

    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByRole("heading", { name: "选择主识别模型" })).toBeInTheDocument();
    expect(screen.getByText("API Key 在应用私有目录中本地加密保存，不调用系统钥匙链。")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("combobox", { name: "语音识别模型" }));
    fireEvent.click(await screen.findByRole("option", { name: "离线模型" }));
    await waitFor(() => expect(patchDictPrefs).toHaveBeenCalledWith({ asrModel: "local-model" }));
    fireEvent.change(screen.getByLabelText("阿里云百炼 API Key"), { target: { value: "test-key" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(updateProviderConfig).toHaveBeenCalledWith("bailian", { apiKey: "test-key" }));

    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByRole("heading", { name: "需要离线使用？" })).toBeInTheDocument();
    expect(screen.getByText("下载后双击文件，或在“设置 → 插件”中选择安装。")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "打开模型下载页" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "打开插件管理" })).toBeInTheDocument();
    expect(screen.queryByText("处理后音量")).not.toBeInTheDocument();
    expect(screen.queryByText("快捷键与输入")).not.toBeInTheDocument();
  });

  it("keeps a fixed dialog and separate footer while replacing each step's scroll region", async () => {
    render(<OnboardingWizard open onClose={() => undefined} />);
    await screen.findByText("麦克风可以正常使用");
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveClass("h-[var(--onboarding-dialog-h)]", "max-h-[85vh]");
    const previous = screen.getByRole("button", { name: "上一步" });
    expect(previous.parentElement).toHaveClass("shrink-0");
    let content = screen.getByRole("region", { name: "权限设置内容" });
    expect(content).toHaveClass("min-h-0", "overflow-y-auto");
    expect(content).not.toContainElement(previous);
    for (const name of ["识别模型设置内容", "离线模型设置内容"]) {
      content.scrollTop = 160;
      fireEvent.click(screen.getByRole("button", { name: "下一步" }));
      const next = screen.getByRole("region", { name });
      expect(next).not.toBe(content);
      expect(next.scrollTop).toBe(0);
      expect(screen.getByRole("dialog")).toBe(dialog);
      expect(screen.getByRole("button", { name: "上一步" })).toBe(previous);
      content = next;
    }
  });
});
