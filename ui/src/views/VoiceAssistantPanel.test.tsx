import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useUiStore } from "@/store/useUiStore";
import { VoiceAssistantView } from "./VoiceAssistantPanel";

const cmd = vi.fn();

vi.mock("@/lib/tauri", () => ({
  CMD: {
    getAppSnapshot: "get_app_snapshot",
    updateAppSettings: "update_app_settings",
    previewAssistant: "preview_assistant",
    getDefaultAssistantPreferences: "get_default_assistant_preferences",
  },
  cmd: (...args: unknown[]) => cmd(...args),
}));

vi.mock("@/features/asr/modelRegistry", () => ({
  useModelCatalogRevision: () => 1,
  optionsForScene: () => [],
}));

vi.mock("@/views/SmartTextPanel", () => ({ SmartTextPanel: () => <div>智能优化设置</div> }));

describe("VoiceAssistantView", () => {
  afterEach(cleanup);
  beforeEach(() => {
    cmd.mockImplementation(async (name: string) => {
      if (name === "get_app_snapshot") {
        return {
          settings: {
            assistantPrefs: {
              translationModel: "none",
              sourceLanguage: "auto",
              targetLanguage: "zh",
              llmProviderId: "default",
              llmModel: "",
              preserveStructure: true,
              answerStyle: "balanced",
              customInstructions: "",
            },
          },
        };
      }
      if (name === "preview_assistant") return "您好：\n请查收新版方案。";
      if (name === "get_default_assistant_preferences") {
        return {
          templateCatalogVersion: 2,
          translationEngine: "llm",
          translationModel: "none",
          sourceLanguage: "auto",
          targetLanguage: "zh",
          translateSpeech: { llmProviderId: "default", llmModel: "", activeTemplateId: "translate-accurate", templates: [], templateTrash: [] },
          editSelection: {
            llmProviderId: "default",
            llmModel: "",
            activeTemplateId: "edit-smart",
            templates: [{ id: "edit-smart", name: "智能执行", prompt: "后端提供的新版内置提示词" }],
            templateTrash: [],
          },
          ask: { llmProviderId: "default", llmModel: "", activeTemplateId: "ask-direct", templates: [], templateTrash: [] },
        };
      }
      return undefined;
    });
    useUiStore.setState({ focusedAssistantAction: null, view: "assistant", assistantTab: "editSelection" });
  });

  it("shows all assistant settings instead of redirecting to dictation basics", async () => {
    render(<VoiceAssistantView />);
    expect(screen.getByRole("heading", { name: "智能助手", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "选区编辑" })).toBeInTheDocument();
    expect(screen.getByText("任务提示词")).toBeInTheDocument();
    expect(screen.getByText("试运行")).toBeInTheDocument();
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("get_app_snapshot"));
  });

  it("runs the configured real model through the safe preview command", async () => {
    render(<VoiceAssistantView />);
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("get_app_snapshot"));
    fireEvent.click(screen.getByRole("button", { name: "运行测试" }));
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("preview_assistant", expect.objectContaining({
        action: "editSelection",
        selectedText: expect.stringContaining("新版方案"),
        spokenText: "改成一封简洁、专业的邮件",
      })));
    expect(await screen.findByText(/试运行完成/)).toBeInTheDocument();
  });

  it("restores built-in templates from the Rust catalog", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<VoiceAssistantView />);
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("get_app_snapshot"));
    fireEvent.click(screen.getByRole("button", { name: "恢复默认" }));
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("get_default_assistant_preferences"));
    expect(cmd).toHaveBeenCalledWith("update_app_settings", expect.objectContaining({
      domain: "assistant",
      value: expect.objectContaining({
        editSelection: expect.objectContaining({
          templates: expect.arrayContaining([expect.objectContaining({ prompt: "后端提供的新版内置提示词" })]),
        }),
      }),
    }));
  });

  it("keeps the shortcut deep link on the exact assistant action", () => {
    useUiStore.getState().openAssistantSettings("ask");
    expect(useUiStore.getState().view).toBe("assistant");
    expect(useUiStore.getState().assistantTab).toBe("ask");
    expect(useUiStore.getState().focusedAssistantAction).toBe("ask");
  });
});
