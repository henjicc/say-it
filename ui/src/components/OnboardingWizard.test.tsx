import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { OnboardingWizard } from "./OnboardingWizard";

const invoke = vi.fn();

vi.mock("@/lib/tauri", () => ({
  CMD: {
    getSetupStatus: "get_setup_status",
    startBackendMic: "start_backend_mic",
    releaseBackendMic: "release_backend_mic",
    startSetupMicMeter: "start_setup_mic_meter",
    getSetupMicLevel: "get_setup_mic_level",
    stopSetupMicMeter: "stop_setup_mic_meter",
  },
  cmd: (...args: unknown[]) => invoke(...args),
}));

vi.mock("@/store/useDictPrefs", () => ({
  useDictPrefs: (selector: (state: { prefs: { micDeviceId: string } }) => unknown) => selector({ prefs: { micDeviceId: "" } }),
}));

const status = {
  onboardingVersion: 0,
  requiredVersion: 1,
  complete: false,
  checks: [
    { id: "microphone", status: "ready", title: "麦克风", message: "检测到输入设备" },
    { id: "provider", status: "ready", title: "识别能力", message: "检测到可用服务" },
    { id: "shortcut", status: "ready", title: "主快捷键", message: "已设置 CapsLock" },
  ],
};

describe("OnboardingWizard", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockImplementation(async (name: string) => {
      if (name === "get_setup_status") return status;
      if (name === "start_backend_mic") return { reused: false };
      if (name === "get_setup_mic_level") return 0.16;
      return undefined;
    });
  });

  it("guides users through recognition, processed audio, and input setup", async () => {
    render(<OnboardingWizard open onClose={() => undefined} />);

    expect(screen.getByRole("heading", { name: "几步完成，说完就能输入" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "开始设置" }));
    expect(await screen.findByText("检测到可用服务")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(await screen.findByText("处理后音量")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("start_setup_mic_meter");

    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByRole("button", { name: "测试" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "测试注入" })).not.toBeInTheDocument();
  });
});
