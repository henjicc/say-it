import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const cmd = vi.fn();
vi.mock("@/lib/tauri", () => ({
  CMD: new Proxy({}, { get: (_target, key) => String(key) }),
  cmd: (...args: unknown[]) => cmd(...args),
  cmdSilent: (...args: unknown[]) => cmd(...args),
}));

const { DataResetSection } = await import("./SettingsAdvancedPanel");

beforeEach(() => {
  cmd.mockReset();
});

afterEach(cleanup);

const openDialog = () => {
  render(<DataResetSection />);
  fireEvent.click(screen.getByText("重置数据并重启"));
  return screen.getByRole("button", { name: "确认重置" });
};

describe("重置数据的确认弹窗", () => {
  /// 失败提示原本写在弹窗**外面**的状态行上，而失败时 pendingReset 不会被清掉：
  /// 弹窗还开着、遮罩把那行字整个挡住。用户按下「确认重置」之后只看到按钮从
  /// 「正在重置…」跳回「确认重置」，完全得不到失败原因。
  it("重置失败时在弹窗里显示原因", async () => {
    cmd.mockRejectedValue(new Error("数据目录被占用"));

    fireEvent.click(openDialog());

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("数据目录被占用");
    // 弹窗必须还开着，用户才能重试或取消。
    expect(screen.getByRole("button", { name: "确认重置" })).toBeEnabled();
  });

  it("关闭弹窗会清掉上一次的失败提示", async () => {
    cmd.mockRejectedValue(new Error("数据目录被占用"));
    fireEvent.click(openDialog());
    await screen.findByRole("alert");

    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());

    fireEvent.click(screen.getByText("重置数据并重启"));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("重置进行中不会重复发起", async () => {
    cmd.mockImplementation(() => new Promise(() => {}));
    const confirm = openDialog();

    fireEvent.click(confirm);
    await waitFor(() => expect(screen.getByRole("button", { name: "正在重置…" })).toBeDisabled());
    expect(cmd).toHaveBeenCalledTimes(1);
  });
});
