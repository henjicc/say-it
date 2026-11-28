import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { hardwareQuery } from "@/features/models/localModelNeeds";
import { resolveDataDirPreview } from "@/features/onboarding/setup";
import { setQueryData } from "@/hooks/useCachedQuery";
import { dataRootQuery } from "@/views/SettingsStartupPanel";
import { OnboardingWizard } from "./OnboardingWizard";

const invoke = vi.fn();
const patchDictPrefs = vi.fn();
const updateConfig = vi.fn();
const setDefault = vi.fn();
const addLlmProvider = vi.fn();
const setMainShortcut = vi.fn();
const setPressHoldMode = vi.fn();

const makeProfile = (id: string, kind: string, displayName: string, extraFields: Array<{ key: string; label: string; secret?: boolean; capabilities?: string[] }> = []) => ({
  id,
  kind,
  displayName,
  authKind: "api-key",
  capabilities: ["asr", "llm"],
  enabled: true,
  configFields: extraFields,
  config: {},
  status: { configured: false, hasApiKey: false, asrConfigured: false, llmConfigured: false },
});

let bailian = makeProfile("bailian", "sdk:bailian", "阿里云百炼");
let volcengine = makeProfile("volcengine", "sdk:volcengine", "火山引擎", [
  { key: "apiKey", label: "API Key", secret: true, capabilities: ["asr"] },
  { key: "arkApiKey", label: "方舟 API Key", secret: true, capabilities: ["llm"] },
]);
let siliconflow = makeProfile("siliconflow", "sdk:siliconflow", "硅基流动");
let groq = makeProfile("llm-groq", "llm:groq", "Groq");

const providerState = {
  profiles: [bailian, volcengine, siliconflow, groq],
  defaults: { asr: "", llm: "", translation: "", ocr: "" },
  load: vi.fn(async () => undefined),
  updateConfig,
  setDefault,
  addLlmProvider,
};

const dictationState = {
  shortcut: { keyCode: "CapsLock", ctrl: false, shift: false, alt: false, meta: false },
  shortcutLabel: "Caps Lock",
  pressHoldMode: false,
};

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(async () => "D:\\"),
}));

vi.mock("@/lib/tauri", () => ({
  CMD: {
    listProviderPlugins: "list_provider_plugins",
    listOfficialModelPacks: "list_official_model_packs",
    cachedOfficialModelPacks: "cached_official_model_packs",
    completeOnboarding: "complete_onboarding",
    detectHardware: "detect_hardware",
    openExternalLink: "open_external_link",
    openPrivacySettings: "open_privacy_settings",
    requestSetupPermissions: "request_setup_permissions",
    getSetupStatus: "get_setup_status",
    microphonePermissionDenied: "microphone_permission_denied",
    getDataRootStatus: "get_data_root_status",
    migrateDataRoot: "migrate_data_root",
    restartApp: "restart_app",
  },
  EVT: { modelPackProgress: "model-pack-progress" },
  cmd: (...args: unknown[]) => invoke(...args),
  cmdSilent: vi.fn(),
  on: vi.fn(async () => () => undefined),
}));

const platform = vi.hoisted(() => ({ isMacOS: false }));
vi.mock("@/lib/platform", () => platform);

const mic = { level: 0.2, startError: "" };
vi.mock("@/features/audio/useMicLevel", () => ({
  useMicLevel: () => mic,
  micLevelFraction: (value: number) => value,
}));

vi.mock("@/features/dictation/hotkeys", () => ({
  shortcutLabel: (combo: { keyCode: string }) => combo.keyCode === "CapsLock" ? "Caps Lock" : combo.keyCode,
  beginShortcutCapture: vi.fn(() => () => undefined),
  setMainShortcut: (...args: unknown[]) => setMainShortcut(...args),
  setPressHoldMode: (...args: unknown[]) => setPressHoldMode(...args),
}));

vi.mock("@/store/useDictationStore", () => ({
  useDictationStore: (selector: (state: unknown) => unknown) => selector(dictationState),
}));

vi.mock("@/features/plugins/pluginInstaller", () => ({ refreshPluginConsumers: vi.fn() }));
vi.mock("@/store/useDictPrefs", () => {
  const state = { prefs: { micDeviceId: "", smartTemplateId: "context-aware-polish" }, patch: (...args: unknown[]) => patchDictPrefs(...args) };
  return { useDictPrefs: Object.assign((selector: (s: unknown) => unknown) => selector(state), { getState: () => state }) };
});
vi.mock("@/store/useProviderStore", () => ({
  useProviderStore: Object.assign((selector: (s: unknown) => unknown) => selector(providerState), { getState: () => providerState }),
}));

function next() {
  fireEvent.click(screen.getByRole("button", { name: /下一步/ }));
}

describe("resolveDataDirPreview", () => {
  it("appends SayItData to drive roots and normal dirs without double nesting", () => {
    const def = "C:\\Users\\example\\AppData\\Local\\say-it";
    expect(resolveDataDirPreview("D:\\", def, false)).toBe("D:\\SayItData");
    expect(resolveDataDirPreview("E:\\", def, false)).toBe("E:\\SayItData");
    expect(resolveDataDirPreview("D:\\Software", def, false)).toBe("D:\\Software\\SayItData");
    expect(resolveDataDirPreview("D:\\SayItData", def, false)).toBe("D:\\SayItData");
    expect(resolveDataDirPreview("D:\\sayitdata\\", def, false)).toBe("D:\\sayitdata");
    expect(resolveDataDirPreview(def, def, false)).toBe(def);
    expect(resolveDataDirPreview("/Volumes/ExternalSSD", "/Users/example/AppSupport", true)).toBe("/Volumes/ExternalSSD/SayItData");
    expect(resolveDataDirPreview("/", "/Users/example/AppSupport", true)).toBe("/SayItData");
  });
});

describe("OnboardingWizard", () => {
  afterEach(cleanup);
  beforeEach(() => {
    platform.isMacOS = false;
    setQueryData(hardwareQuery, undefined as never);
    setQueryData(dataRootQuery, undefined as never);
    bailian = makeProfile("bailian", "sdk:bailian", "阿里云百炼");
    volcengine = makeProfile("volcengine", "sdk:volcengine", "火山引擎", [
      { key: "apiKey", label: "API Key", secret: true, capabilities: ["asr"] },
      { key: "arkApiKey", label: "方舟 API Key", secret: true, capabilities: ["llm"] },
    ]);
    siliconflow = makeProfile("siliconflow", "sdk:siliconflow", "硅基流动");
    groq = makeProfile("llm-groq", "llm:groq", "Groq");
    providerState.profiles = [bailian, volcengine, siliconflow, groq];
    providerState.defaults = { asr: "", llm: "", translation: "", ocr: "" };

    dictationState.shortcut = { keyCode: "CapsLock", ctrl: false, shift: false, alt: false, meta: false };
    dictationState.pressHoldMode = false;

    invoke.mockReset().mockImplementation(async (name: string) => {
      if (name === "list_provider_plugins") return { apiVersion: 5, plugins: [], errors: [] };
      if (name === "list_official_model_packs") return [];
      if (name === "get_data_root_status") {
        return {
          activeRoot: "C:\\Users\\example\\AppData\\Local\\say-it",
          configuredRoot: "C:\\Users\\example\\AppData\\Local\\say-it",
          defaultRoot: "C:\\Users\\example\\AppData\\Local\\say-it",
          isCustom: false,
          restartRequired: false,
        };
      }
      if (name === "detect_hardware") {
        return { gpuName: null, gpuMemoryMb: 0, memoryMb: 16384, unifiedMemory: false };
      }
      return undefined;
    });
    patchDictPrefs.mockReset().mockResolvedValue(undefined);
    setDefault.mockReset().mockResolvedValue(undefined);
    setMainShortcut.mockReset().mockResolvedValue(undefined);
    setPressHoldMode.mockReset().mockResolvedValue(undefined);
    updateConfig.mockReset().mockImplementation(async (id: string) => {
      const target = providerState.profiles.find((p) => p.id === id);
      if (target) target.status = { configured: true, hasApiKey: true, asrConfigured: true, llmConfigured: true };
      return target;
    });
    addLlmProvider.mockReset().mockImplementation(async (payload: { adapter: string; displayName: string }) => {
      const added = makeProfile(`llm-${payload.adapter}`, `llm:${payload.adapter}`, payload.displayName);
      added.status = { configured: true, hasApiKey: true, asrConfigured: false, llmConfigured: true };
      providerState.profiles.push(added);
      return added;
    });
    Object.assign(mic, { level: 0.2, startError: "" });
  });

  it("hides macOS permissions on Windows and shows all 3 real permissions on macOS", async () => {
    const { unmount } = render(<OnboardingWizard open required onClose={vi.fn()} />);
    expect(screen.queryByText("macOS 系统权限")).toBeNull();
    expect(screen.getByText(/Windows 无需额外系统授权/)).toBeTruthy();
    unmount();

    platform.isMacOS = true;
    invoke.mockImplementation(async (name: string) => {
      if (name === "get_setup_status") {
        return {
          checks: [
            { id: "permissions", title: "辅助功能", message: "", status: "blocked", action: "permissions" },
            { id: "inputMonitoring", title: "输入监控", message: "", status: "blocked", action: "privacy:input-monitoring" },
            { id: "screenRecording", title: "屏幕录制权限", message: "", status: "blocked", action: "privacy:screen-recording" },
          ],
        };
      }
      if (name === "get_data_root_status") {
        return {
          activeRoot: "/Users/example/Library/Application Support/say-it",
          configuredRoot: "/Users/example/Library/Application Support/say-it",
          defaultRoot: "/Users/example/Library/Application Support/say-it",
          isCustom: false,
          restartRequired: false,
        };
      }
      return undefined;
    });
    render(<OnboardingWizard open required onClose={vi.fn()} />);
    expect(await screen.findByText("macOS 系统权限")).toBeTruthy();
    expect(screen.getByText("辅助功能")).toBeTruthy();
    expect(screen.getByText("输入监控")).toBeTruthy();
    expect(await screen.findByText("允许后要重启说吧！才生效")).toBeTruthy();
    const openBtns = screen.getAllByRole("button", { name: "打开系统设置" });
    fireEvent.click(openBtns[1]);
    expect(invoke).toHaveBeenCalledWith("open_privacy_settings", { kind: "screen-recording" });
  });

  it("points to system settings when microphone permission was denied", async () => {
    Object.assign(mic, { level: 0, startError: "" });
    invoke.mockImplementation(async (name: string) => name === "microphone_permission_denied" ? true : undefined);
    render(<OnboardingWizard open required onClose={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "打开系统设置" }));
    expect(invoke).toHaveBeenCalledWith("open_privacy_settings", { kind: "microphone" });
    invoke.mockImplementation(async () => false);
    fireEvent.focus(window);
    await waitFor(() => expect(screen.queryByRole("button", { name: "打开系统设置" })).toBeNull());
  });

  it("previews SayItData subfolder and migrates data root from Step 1", async () => {
    invoke.mockImplementation(async (name: string, args?: { target?: string }) => {
      if (name === "get_data_root_status") {
        return {
          activeRoot: "C:\\Users\\example\\AppData\\Local\\say-it",
          configuredRoot: "C:\\Users\\example\\AppData\\Local\\say-it",
          defaultRoot: "C:\\Users\\example\\AppData\\Local\\say-it",
          isCustom: false,
          restartRequired: false,
        };
      }
      if (name === "migrate_data_root") {
        expect(args?.target).toBe("D:\\");
        return {
          activeRoot: "D:\\SayItData",
          configuredRoot: "D:\\SayItData",
          defaultRoot: "C:\\Users\\example\\AppData\\Local\\say-it",
          isCustom: true,
          restartRequired: false,
        };
      }
      return undefined;
    });
    render(<OnboardingWizard open required onClose={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "更改" }));
    const input = await screen.findByLabelText("目录路径");
    fireEvent.change(input, { target: { value: "D:\\" } });
    expect(screen.getByText("D:\\SayItData")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "确定使用此目录" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("migrate_data_root", { target: "D:\\" }));
  });

  it("can only be finished when required, and can be closed when optional", () => {
    const onClose = vi.fn();
    const { rerender } = render(<OnboardingWizard open required onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: /上一步|关闭/ })).toBeNull();

    rerender(<OnboardingWizard open required={false} onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(onClose).toHaveBeenCalledTimes(2);
    expect(invoke).not.toHaveBeenCalledWith("complete_onboarding");
  });

  it("configures Bailian ASR + LLM in one step from online bundle", async () => {
    render(<OnboardingWizard open required onClose={vi.fn()} />);
    next();
    expect(screen.getByText(/本软件永久免费，仅由服务商按量计费/)).toBeTruthy();
    next();
    const nextBtn = screen.getByRole("button", { name: /下一步/ }) as HTMLButtonElement;
    expect(nextBtn.disabled).toBe(true);
    expect(screen.getByText("填好密钥即可继续")).toBeTruthy();

    fireEvent.change(screen.getByLabelText("阿里云百炼 百炼 API Key"), { target: { value: " sk-bailian " } });
    next();

    await waitFor(() => expect(updateConfig).toHaveBeenCalledWith("bailian", expect.objectContaining({ apiKey: "sk-bailian", model: expect.any(String) })));
    expect(setDefault).toHaveBeenCalledWith("llm", "bailian");
    expect(patchDictPrefs).toHaveBeenCalledWith({
      asrModel: "fun-asr-realtime-2026-02-28",
      smartProcessingEnabled: true,
      smartLlmProviderId: "default",
      smartLlmModel: "",
    });
    expect(await screen.findByText("设置你的唤起快捷键")).toBeTruthy();
  });

  it("offers full local plan on capable GPU (0.6B default, switchable to 1.7B) and applies local polish", async () => {
    invoke.mockImplementation(async (name: string) => {
      if (name === "detect_hardware") return { gpuName: "RTX 4090", gpuMemoryMb: 24564, memoryMb: 65536, unifiedMemory: false };
      if (name === "list_provider_plugins") {
        return {
          apiVersion: 5,
          errors: [],
          plugins: ["official.transcribe-qwen3-asr-0.6b", "official.transcribe-qwen3-asr-1.7b", "official.llama-polish-4b"]
            .map((id) => ({ id, modelPack: { state: "ready" } })),
        };
      }
      if (name === "list_official_model_packs") return [];
      if (name === "get_data_root_status") {
        return {
          activeRoot: "D:\\SayItData",
          configuredRoot: "D:\\SayItData",
          defaultRoot: "C:\\Users\\example\\AppData\\Local\\say-it",
          isCustom: true,
          restartRequired: false,
        };
      }
      return undefined;
    });

    render(<OnboardingWizard open required onClose={vi.fn()} />);
    next();
    await waitFor(() => expect(screen.getByRole("radio", { name: /离线模式/ }).getAttribute("aria-checked")).toBe("true"));
    next();
    expect(screen.getByRole("radio", { name: /全能本地/ }).getAttribute("aria-checked")).toBe("true");
    await screen.findByText("全部就绪");
    fireEvent.click(screen.getByRole("button", { name: "1.7B 高准" }));
    next();

    await waitFor(() => expect(patchDictPrefs).toHaveBeenCalledWith({
      asrModel: "local-qwen3-asr-1.7b-sentence-q5",
      smartProcessingEnabled: true,
      smartLlmProviderId: "local-polish-4b",
      smartLlmModel: "",
    }));
  });

  it("disables full local plan without discrete GPU and defaults Step 4 to CapsLock toggle", async () => {
    invoke.mockImplementation(async (name: string) => {
      if (name === "detect_hardware") return { gpuName: null, gpuMemoryMb: 0, memoryMb: 16384, unifiedMemory: false };
      if (name === "list_provider_plugins") {
        return {
          apiVersion: 5,
          errors: [],
          plugins: ["official.sherpa-funasr-nano-ctc", "official.sherpa-ct-punct-zh-en"]
            .map((id) => ({ id, modelPack: { state: "ready" } })),
        };
      }
      if (name === "list_official_model_packs") return [];
      if (name === "get_data_root_status") {
        return {
          activeRoot: "C:\\Users\\example\\AppData\\Local\\say-it",
          configuredRoot: "C:\\Users\\example\\AppData\\Local\\say-it",
          defaultRoot: "C:\\Users\\example\\AppData\\Local\\say-it",
          isCustom: false,
          restartRequired: false,
        };
      }
      return undefined;
    });

    const onClose = vi.fn();
    render(<OnboardingWizard open required onClose={onClose} />);
    next();
    await waitFor(() => expect(screen.getByRole("radio", { name: /在线服务/ }).getAttribute("aria-checked")).toBe("true"));
    fireEvent.click(screen.getByRole("radio", { name: /离线模式/ }));
    next();
    await waitFor(() => expect(screen.getByRole("radio", { name: /全能本地/ }).getAttribute("aria-disabled")).toBe("true"));
    expect(screen.getByRole("radio", { name: /轻量极速/ }).getAttribute("aria-checked")).toBe("true");
    await screen.findByText("全部就绪");
    next();

    await waitFor(() => expect(patchDictPrefs).toHaveBeenCalledWith({
      asrModel: "local-funasr-nano-sentence-int8",
      smartProcessingEnabled: false,
      localRulesEnabled: true,
    }));

    expect(await screen.findByRole("radio", { name: "按一下（默认）" })).toBeTruthy();
    expect(screen.getByRole("radio", { name: "按一下（默认）" }).getAttribute("aria-checked")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "右 Alt" }));
    expect(setMainShortcut).toHaveBeenCalledWith({ keyCode: "AltRight", ctrl: false, shift: false, alt: false, meta: false });
    fireEvent.click(screen.getByRole("radio", { name: "按住说话" }));
    expect(setPressHoldMode).toHaveBeenCalledWith(true);

    fireEvent.click(screen.getByRole("button", { name: "开始使用" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("complete_onboarding"));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });
});
