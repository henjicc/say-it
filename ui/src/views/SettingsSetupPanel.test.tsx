import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CMD, cmd, type SetupStatus } from "@/lib/tauri";
import { SettingsSetupPanel } from "./SettingsSetupPanel";

vi.mock("@/lib/tauri", () => ({ CMD: { getSetupStatus: "get_setup_status" }, cmd: vi.fn() }));

const blocked: SetupStatus = {
  onboardingVersion: 1, requiredVersion: 1, complete: true,
  checks: [
    { id: "microphone", title: "麦克风", message: "检测到输入设备", status: "ready" },
    { id: "provider", title: "识别能力", message: "尚未配置认证信息", status: "blocked" },
  ],
};
const ready: SetupStatus = {
  ...blocked,
  checks: blocked.checks.map((check) => ({ ...check, status: "ready", message: "检查通过" })),
};

describe("SettingsSetupPanel", () => {
  beforeEach(() => vi.mocked(cmd).mockReset().mockResolvedValue(blocked));
  afterEach(cleanup);

  it("keeps environment checks collapsed and preserves the existing guide entry", () => {
    render(<SettingsSetupPanel />);
    const toggle = screen.getByRole("button", { name: "环境状态" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(document.getElementById(toggle.getAttribute("aria-controls")!)).not.toBeVisible();
    expect(cmd).not.toHaveBeenCalled();
    const openGuide = vi.fn();
    window.addEventListener("sayit-open-setup", openGuide);
    fireEvent.click(screen.getByRole("button", { name: "重新运行使用引导" }));
    expect(openGuide).toHaveBeenCalledTimes(1);
    window.removeEventListener("sayit-open-setup", openGuide);
  });

  it("reads details on demand and fetches current results after refreshing or reopening", async () => {
    render(<SettingsSetupPanel />);
    const toggle = screen.getByRole("button", { name: "环境状态" });
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("status")).toHaveTextContent("正在检查环境");
    expect(screen.getByRole("button", { name: "重新检查" })).toBeDisabled();
    expect(await screen.findByText("尚未配置认证信息")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("1 项需要处理");
    expect(cmd).toHaveBeenCalledWith(CMD.getSetupStatus);
    vi.mocked(cmd).mockResolvedValue(ready);
    fireEvent.click(screen.getByRole("button", { name: "重新检查" }));
    expect(await screen.findByText("所有关键能力均可用")).toBeInTheDocument();
    fireEvent.click(toggle);
    expect(screen.queryByRole("list")).not.toBeInTheDocument();
    fireEvent.click(toggle);
    await screen.findByText("所有关键能力均可用");
    expect(cmd).toHaveBeenCalledTimes(3);
  });

  it("shows check failures and allows retry without leaving stale success feedback", async () => {
    vi.mocked(cmd).mockRejectedValueOnce(new Error("检测服务不可用"));
    render(<SettingsSetupPanel />);
    fireEvent.click(screen.getByRole("button", { name: "环境状态" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("检测服务不可用");
    expect(screen.getByRole("status")).toHaveTextContent("环境检查失败");
    fireEvent.click(screen.getByRole("button", { name: "重新检查" }));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
    expect(await screen.findByText("尚未配置认证信息")).toBeInTheDocument();
  });

  it("ignores an earlier check that finishes after its panel was closed", async () => {
    let resolveOld!: (value: SetupStatus) => void;
    vi.mocked(cmd).mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; }));
    render(<SettingsSetupPanel />);
    const toggle = screen.getByRole("button", { name: "环境状态" });
    fireEvent.click(toggle);
    fireEvent.click(toggle);
    fireEvent.click(toggle);
    await screen.findByText("尚未配置认证信息");
    await act(async () => resolveOld(ready));
    expect(screen.getByRole("status")).toHaveTextContent("1 项需要处理");
  });
});
