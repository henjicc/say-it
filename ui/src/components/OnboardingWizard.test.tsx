import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { OnboardingWizard } from "./OnboardingWizard";

const invoke = vi.fn();
const patchDictPrefs = vi.fn();
const updateConfig = vi.fn();
const setDefault = vi.fn();
const addLlmProvider = vi.fn();

const bailian = {
  id: "bailian",
  kind: "sdk:bailian",
  displayName: "阿里云百炼",
  authKind: "api-key",
  capabilities: ["asr", "llm"],
  enabled: true,
  configFields: [],
  status: { configured: false, hasApiKey: false },
};

const providerState = {
  profiles: [bailian],
  defaults: { asr: "", llm: "", translation: "", ocr: "" },
  load: vi.fn(async () => undefined),
  updateConfig,
  setDefault,
  addLlmProvider,
};

vi.mock("@/lib/tauri", () => ({
  CMD: {
    listProviderPlugins: "list_provider_plugins",
    listOfficialModelPacks: "list_official_model_packs",
    completeOnboarding: "complete_onboarding",
    openExternalLink: "open_external_link",
  },
  EVT: { modelPackProgress: "model-pack-progress" },
  cmd: (...args: unknown[]) => invoke(...args),
  cmdSilent: vi.fn(),
  on: vi.fn(async () => () => undefined),
}));
vi.mock("@/lib/platform", () => ({ isMacOS: false }));
vi.mock("@/features/audio/useMicLevel", () => ({
  useMicLevel: () => ({ level: 0.2, startError: "" }),
  micLevelFraction: () => 0.2,
}));
vi.mock("@/views/dictation/DictationSections", () => ({
  MicRow: () => null,
  ShortcutRows: () => null,
  OrbGestureSection: () => null,
}));
vi.mock("@/features/dictation/hotkeys", () => ({ shortcutLabel: () => "CapsLock" }));
vi.mock("@/store/useDictationStore", () => ({
  useDictationStore: (selector: (state: unknown) => unknown) => selector({ shortcut: { keyCode: "CapsLock" } }),
}));
vi.mock("@/features/plugins/pluginInstaller", () => ({ refreshPluginConsumers: vi.fn() }));
vi.mock("@/store/useDictPrefs", () => {
  const state = { prefs: { micDeviceId: "" }, patch: (...args: unknown[]) => patchDictPrefs(...args) };
  return { useDictPrefs: Object.assign((selector: (s: unknown) => unknown) => selector(state), { getState: () => state }) };
});
vi.mock("@/store/useProviderStore", () => ({
  useProviderStore: Object.assign((selector: (s: unknown) => unknown) => selector(providerState), { getState: () => providerState }),
}));

function next() {
  fireEvent.click(screen.getByRole("button", { name: "下一步" }));
}

describe("OnboardingWizard", () => {
  afterEach(cleanup);
  beforeEach(() => {
    invoke.mockReset().mockImplementation(async (name: string) => {
      if (name === "list_provider_plugins") return { apiVersion: 5, plugins: [], errors: [] };
      if (name === "list_official_model_packs") return [];
      return undefined;
    });
    patchDictPrefs.mockReset().mockResolvedValue(undefined);
    setDefault.mockReset().mockResolvedValue(undefined);
    updateConfig.mockReset().mockImplementation(async () => {
      bailian.status = { configured: true, hasApiKey: true };
      return bailian;
    });
    bailian.status = { configured: false, hasApiKey: false };
    providerState.defaults.llm = "";
  });

  it("can only be finished, and the first step has no way back", () => {
    render(<OnboardingWizard open required onClose={vi.fn()} />);
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(screen.getByRole("dialog").parentElement!);
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /上一步|关闭|稍后设置/ })).toBeNull();
    expect(invoke).not.toHaveBeenCalledWith("complete_onboarding");
  });

  it("can be closed when re-run from settings", () => {
    const onClose = vi.fn();
    render(<OnboardingWizard open required={false} onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(onClose).toHaveBeenCalledTimes(2);
    expect(invoke).not.toHaveBeenCalledWith("complete_onboarding");
  });

  it("needs a recognition key before continuing and saves it with the model", async () => {
    render(<OnboardingWizard open required onClose={vi.fn()} />);
    next();
    const button = screen.getByRole("button", { name: "下一步" }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    expect(screen.getByText("填好密钥后继续")).toBeTruthy();

    fireEvent.change(screen.getByLabelText("阿里云百炼 API Key"), { target: { value: " sk-test " } });
    next();

    await waitFor(() => expect(patchDictPrefs).toHaveBeenCalledWith({ asrModel: "fun-asr-realtime-2026-02-28" }));
    expect(updateConfig).toHaveBeenCalledWith("bailian", { apiKey: "sk-test" });
    expect(await screen.findByText("沿用识别模型的百炼密钥")).toBeTruthy();
  });

  it("reuses the Bailian key for smart cleanup and makes it the general model", async () => {
    bailian.status = { configured: true, hasApiKey: true };
    render(<OnboardingWizard open required onClose={vi.fn()} />);
    next();
    next();
    await screen.findByText("沿用识别模型的百炼密钥");
    expect(screen.queryByLabelText("阿里云百炼 API Key")).toBeNull();
    next();

    await waitFor(() => expect(patchDictPrefs).toHaveBeenCalledWith({
      smartProcessingEnabled: true,
      smartLlmProviderId: "default",
      smartLlmModel: "",
    }));
    expect(updateConfig).toHaveBeenCalledWith("bailian", { model: expect.any(String) });
    expect(setDefault).toHaveBeenCalledWith("llm", "bailian");
  });

  it("saves completion before closing", async () => {
    bailian.status = { configured: true, hasApiKey: true };
    let save: (() => void) | undefined;
    invoke.mockImplementation(async (name: string) => {
      if (name === "complete_onboarding") return new Promise<void>((resolve) => { save = resolve; });
      if (name === "list_provider_plugins") return { apiVersion: 5, plugins: [], errors: [] };
      if (name === "list_official_model_packs") return [];
    });
    const onClose = vi.fn();
    render(<OnboardingWizard open required onClose={onClose} />);
    next();
    next();
    await screen.findByText("沿用识别模型的百炼密钥");
    next();
    fireEvent.click(await screen.findByRole("button", { name: "开始使用" }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("complete_onboarding"));
    expect(onClose).not.toHaveBeenCalled();
    save!();
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });
});
