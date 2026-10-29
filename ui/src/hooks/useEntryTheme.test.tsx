import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const cmd = vi.fn();
const themeListeners: Array<(payload: unknown) => void> = [];
vi.mock("@/lib/tauri", () => ({
  CMD: new Proxy({}, { get: (_target, key) => String(key) }),
  EVT: { themeChanged: "theme-changed" },
  cmd: (...args: unknown[]) => cmd(...args),
  cmdSilent: (...args: unknown[]) => cmd(...args),
  on: async (event: string, handler: (payload: unknown) => void) => {
    if (event === "theme-changed") themeListeners.push(handler);
    return () => undefined;
  },
}));

const { useEntryTheme } = await import("./useEntryTheme");
const { AssistantAnswerApp } = await import("@/assistant");
const { ContextDebugApp } = await import("@/context-debug/ContextDebugApp");

/**
 * 主题是运行时切换的：亮色令牌靠 `<html data-ui-tone="light">` 生效，强调色靠覆写
 * `--color-*` 生效。主窗口以外的独立入口过去从不初始化主题，于是亮色主题下整窗仍按
 * 暗色令牌渲染——深底深字，开启系统毛玻璃后面板还会不透明。
 */
const lightSnapshot = { settings: { theme: { tone: "light", accent: "#3B82F6" } } };
const emptyAnswer = {
  text: "",
  reasoning: "",
  sourceText: "",
  canInsert: false,
  streaming: false,
  pinned: false,
};

beforeEach(() => {
  cmd.mockReset();
  themeListeners.length = 0;
  delete document.documentElement.dataset.uiTone;
  cmd.mockImplementation(async (name: string) => {
    if (name === "getAppSnapshot") return lightSnapshot;
    if (name === "getAssistantAnswer") return emptyAnswer;
    return undefined;
  });
});

afterEach(cleanup);

function Bare() {
  useEntryTheme();
  return null;
}

describe("独立入口的主题初始化", () => {
  it("助手回答窗按已保存的主题渲染", async () => {
    render(<AssistantAnswerApp />);
    await waitFor(() => expect(document.documentElement.dataset.uiTone).toBe("light"));
  });

  it("上下文调试窗按已保存的主题渲染", async () => {
    render(<ContextDebugApp />);
    await waitFor(() => expect(document.documentElement.dataset.uiTone).toBe("light"));
  });

  it("之后的主题变更也跟随", async () => {
    render(<Bare />);
    await waitFor(() => expect(document.documentElement.dataset.uiTone).toBe("light"));
    await waitFor(() => expect(themeListeners.length).toBe(1));

    themeListeners[0]({ tone: "dark", accent: "#3B82F6" });

    expect(document.documentElement.dataset.uiTone).toBe("dark");
  });
});
