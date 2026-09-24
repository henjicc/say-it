import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ShortcutRecorder } from "./ShortcutRecorder";
import { isCapturing, type ShortcutCombo } from "./hotkeys";

vi.mock("@/lib/tauri", () => ({
  CMD: { setHotkeyCapturing: "set_hotkey_capturing" },
  cmd: vi.fn(),
  cmdSilent: vi.fn(),
}));

afterEach(cleanup);

const empty: ShortcutCombo = { keyCode: "", ctrl: false, shift: false, alt: false, meta: false };
const saved: ShortcutCombo = { ...empty, keyCode: "CapsLock" };

describe("快捷键设置组件", () => {
  it("空值可开始录制，组合键只在完整按下后保存", () => {
    const onChange = vi.fn();
    render(<ShortcutRecorder value={empty} onChange={onChange} />);
    fireEvent.click(screen.getByRole("button", { name: "快捷键：点击设置" }));
    fireEvent.keyDown(window, { code: "ControlLeft", ctrlKey: true });
    expect(onChange).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { code: "KeyK", ctrlKey: true });
    expect(onChange).toHaveBeenCalledExactlyOnceWith({ ...empty, keyCode: "KeyK", ctrl: true });
    expect(isCapturing()).toBe(false);
  });

  it("右侧按钮重新录制而不清除，Esc 保留原值", () => {
    const onChange = vi.fn();
    const onClear = vi.fn();
    render(<ShortcutRecorder value={saved} onChange={onChange} onClear={onClear} />);
    fireEvent.click(screen.getByRole("button", { name: "重新录制快捷键" }));
    expect(isCapturing()).toBe(true);
    fireEvent.keyDown(window, { code: "Escape" });
    expect(screen.getByRole("button", { name: "快捷键：Caps Lock" })).toBeVisible();
    expect(onChange).not.toHaveBeenCalled();
    expect(onClear).not.toHaveBeenCalled();
    expect(isCapturing()).toBe(false);
  });

  it("Delete 在未录制时清除，录制时仍可作为快捷键", () => {
    const onChange = vi.fn();
    const onClear = vi.fn();
    render(<ShortcutRecorder value={saved} onChange={onChange} onClear={onClear} />);
    const field = screen.getByRole("button", { name: "快捷键：Caps Lock" });
    fireEvent.keyDown(field, { key: "Delete", code: "Delete" });
    expect(onClear).toHaveBeenCalledTimes(1);
    fireEvent.click(field);
    fireEvent.keyDown(field, { key: "Delete", code: "Delete" });
    expect(onChange).toHaveBeenCalledExactlyOnceWith({ ...empty, keyCode: "Delete" });
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it("录制中切换到禁用或卸载时释放热键捕获", () => {
    const onChange = vi.fn();
    const { rerender, unmount } = render(<ShortcutRecorder value={saved} onChange={onChange} />);
    fireEvent.click(screen.getByRole("button", { name: "重新录制快捷键" }));
    rerender(<ShortcutRecorder value={saved} onChange={onChange} disabled />);
    expect(isCapturing()).toBe(false);
    expect(screen.getByRole("button", { name: "重新录制快捷键" })).toBeDisabled();
    rerender(<ShortcutRecorder value={saved} onChange={onChange} />);
    fireEvent.click(screen.getByRole("button", { name: "重新录制快捷键" }));
    unmount();
    expect(isCapturing()).toBe(false);
    fireEvent.keyDown(window, { code: "KeyK" });
    expect(onChange).not.toHaveBeenCalled();
  });

  it("离开组件或窗口时取消，不把后续输入录入快捷键", () => {
    const onChange = vi.fn();
    render(<ShortcutRecorder value={saved} onChange={onChange} />);
    const field = screen.getByRole("button", { name: "快捷键：Caps Lock" });
    fireEvent.click(field);
    fireEvent.blur(field, { relatedTarget: document.body });
    expect(isCapturing()).toBe(false);
    fireEvent.click(field);
    fireEvent.blur(window);
    expect(isCapturing()).toBe(false);
    fireEvent.keyDown(window, { code: "KeyK" });
    expect(onChange).not.toHaveBeenCalled();
  });
});
