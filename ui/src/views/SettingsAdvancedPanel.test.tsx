import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DiagnosticSection } from "./SettingsAdvancedPanel";

const cmd = vi.fn();
const saveDialog = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: (...args: unknown[]) => saveDialog(...args) }));
vi.mock("@/lib/tauri", () => ({
  CMD: {
    getAppSnapshot: "get_app_snapshot",
    getDiagnosticStatus: "get_diagnostic_status",
    updateAppSettings: "update_app_settings",
    setContentDiagnostics: "set_content_diagnostics",
    clearDiagnosticLogs: "clear_diagnostic_logs",
    openDiagnosticDirectory: "open_diagnostic_directory",
    exportDiagnosticBundle: "export_diagnostic_bundle",
  },
  cmd: (...args: unknown[]) => cmd(...args),
  cmdSilent: vi.fn(),
}));
vi.mock("@/store/useDictPrefs", () => ({ useDictPrefs: vi.fn() }));
vi.mock("@/store/useAudioStore", () => ({ useAudioStore: vi.fn() }));
vi.mock("@/store/useSubtitleStore", () => ({ useSubtitleStore: vi.fn(), parseSubtitleSource: vi.fn() }));
vi.mock("@/lib/audio-dsp", () => ({ dspDefaults: {} }));
vi.mock("@/features/audio/lab", () => ({}));

describe("DiagnosticSection", () => {
  afterEach(cleanup);
  beforeEach(() => {
    cmd.mockReset();
    saveDialog.mockReset();
    saveDialog.mockResolvedValue("/tmp/say-it-diagnostics.zip");
    cmd.mockImplementation((command: string) => {
      if (command === "get_app_snapshot") return Promise.resolve({ settings: { diagnosticsPrefs: { verboseLogging: false } } });
      if (command === "get_diagnostic_status") return Promise.resolve({ directory: "/tmp/logs", verboseLogging: false, contentLoggingEnabled: false, contentLoggingRemainingSeconds: 0 });
      if (command === "set_content_diagnostics") return Promise.resolve({ directory: "/tmp/logs", verboseLogging: false, contentLoggingEnabled: true, contentLoggingRemainingSeconds: 1800 });
      return Promise.resolve(undefined);
    });
  });

  it("exports redacted diagnostics by default and visibly warns before including content", async () => {
    render(<DiagnosticSection />);
    const exportButton = await screen.findByRole("button", { name: "导出诊断包" });
    fireEvent.click(exportButton);
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("export_diagnostic_bundle", {
      destination: "/tmp/say-it-diagnostics.zip",
      includeContent: false,
    }));
    fireEvent.click(screen.getByRole("checkbox", { name: "包含正文日志" }));
    expect(screen.getByRole("alert")).toHaveTextContent("包含输入文本");
  });

  it("requires confirmation before starting the temporary content log", async () => {
    render(<DiagnosticSection />);
    const toggle = await screen.findByRole("switch", { name: "临时正文日志" });

    // 确认框必须是应用内 Modal：window.confirm 在这套无边框窗口里不呈现，
    // 用它等于没有确认。未点确认之前，命令不得下发。
    fireEvent.click(toggle);
    expect(await screen.findByText("开启临时正文日志")).toBeInTheDocument();
    expect(cmd).not.toHaveBeenCalledWith("set_content_diagnostics", { enabled: true });

    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(cmd).not.toHaveBeenCalledWith("set_content_diagnostics", { enabled: true });

    fireEvent.click(toggle);
    fireEvent.click(await screen.findByRole("button", { name: "开启 30 分钟" }));
    await waitFor(() => expect(cmd).toHaveBeenCalledWith("set_content_diagnostics", { enabled: true }));
  });
});
