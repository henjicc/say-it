import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { OnboardingWizard } from "./OnboardingWizard";

vi.mock("@/lib/tauri", () => ({
  CMD: { getSetupStatus: "get_setup_status" },
  cmd: vi.fn(async () => ({ onboardingVersion: 0, requiredVersion: 1, complete: false, checks: [{ id: "microphone", status: "ready", title: "麦克风", message: "检测到输入设备" }] })),
}));

describe("OnboardingWizard", () => {
  it("shows concrete environment diagnostics", async () => {
    render(<OnboardingWizard open onClose={() => undefined} />);
    expect(await screen.findByText("检测到输入设备")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "测试注入" })).toBeInTheDocument();
  });
});
