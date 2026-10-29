import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Tooltip, HelpLabel } from "./Tooltip";
import { Field } from "./Field";
import { Modal } from "./Modal";

describe("说明提示", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => { cleanup(); vi.useRealTimers(); });
  const advance = (ms: number) => act(() => vi.advanceTimersByTime(ms));
  const focus = (element: HTMLElement) => act(() => element.focus());
  const finishAnimation = (element: HTMLElement) => {
    // jsdom 没有 AnimationEvent，React 会监听 WebKit 前缀事件。
    const eventName = "AnimationEvent" in window ? "animationend" : "webkitAnimationEnd";
    fireEvent(element, new Event(eventName, { bubbles: true }));
  };

  it("默认不展示说明或帮助图标，聚焦满半秒后通过独立浮层显示", () => {
    const view = render(<HelpLabel content="帮助识别常用人名">热词表</HelpLabel>);
    expect(screen.queryByText("帮助识别常用人名")).not.toBeInTheDocument();
    expect(view.container.querySelector("svg")).toBeNull();
    focus(screen.getByText("热词表"));
    advance(499);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    advance(1);
    const tip = screen.getByRole("tooltip");
    expect(tip).toHaveAttribute("data-state", "open");
    expect(tip).toHaveTextContent("帮助识别常用人名");
    expect(view.container).not.toContainElement(tip);
    expect(screen.getByText("热词表")).toHaveAttribute("aria-describedby", tip.id);
  });

  it("关闭时保留浮层直到淡出结束，重新打开后不被旧动画卸载", () => {
    render(<HelpLabel content="说明">选项</HelpLabel>);
    const label = screen.getByText("选项");
    focus(label);
    advance(500);
    const tip = screen.getByRole("tooltip");
    fireEvent.keyDown(label, { key: "Escape" });
    expect(tip).toHaveAttribute("data-state", "closed");
    expect(tip).toHaveAttribute("aria-hidden", "true");
    expect(tip).toBeInTheDocument();
    expect(label).not.toHaveAttribute("aria-describedby");

    act(() => label.blur());
    focus(label);
    advance(500);
    finishAnimation(tip);
    expect(screen.getByRole("tooltip")).toBe(tip);
    expect(tip).toHaveAttribute("data-state", "open");

    fireEvent.keyDown(label, { key: "Escape" });
    finishAnimation(tip);
    expect(tip).not.toBeInTheDocument();
  });

  it("字段获得焦点后关联说明，风险信息始终显示", () => {
    render(<Field label="热词" hint="常用人名" message="获取会替换当前列表" controlId="hotword"><input id="hotword" /></Field>);
    const input = screen.getByRole("textbox", { name: "热词" });
    expect(screen.getByRole("status")).toHaveTextContent("获取会替换当前列表");
    focus(input);
    advance(500);
    expect(input).toHaveAccessibleDescription("常用人名");
    fireEvent.keyDown(input, { key: "Escape" });
    expect(input).not.toHaveAttribute("aria-describedby");
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("第一次 Esc 只关闭说明，第二次才关闭所在弹窗", () => {
    const close = vi.fn();
    render(<Modal open onClose={close} title="设置"><HelpLabel content="说明">选项</HelpLabel></Modal>);
    const label = screen.getByText("选项");
    focus(label);
    advance(500);
    fireEvent.keyDown(label, { key: "Escape" });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    expect(close).not.toHaveBeenCalled();
    fireEvent.keyDown(label, { key: "Escape" });
    expect(close).toHaveBeenCalledOnce();
  });

  it("提前离焦、滚动或卸载都会取消待显示的提示", () => {
    const view = render(<HelpLabel content="说明">选项</HelpLabel>);
    const label = screen.getByText("选项");
    focus(label);
    act(() => label.blur());
    advance(600);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    focus(label);
    fireEvent.scroll(document);
    advance(600);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    act(() => label.blur());
    focus(label);
    view.unmount();
    advance(600);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("不覆盖现有事件和描述，浮层在窗口底部换到上方", () => {
    const onFocus = vi.fn();
    render(<><span id="existing">原有说明</span><Tooltip content="补充说明"><button aria-describedby="existing" onFocus={onFocus}>选项</button></Tooltip></>);
    const button = screen.getByRole("button");
    vi.spyOn(button, "getBoundingClientRect").mockReturnValue({ x: 990, y: 750, left: 990, top: 750, right: 1020, bottom: 780, width: 30, height: 30, toJSON: () => ({}) });
    focus(button);
    advance(500);
    expect(onFocus).toHaveBeenCalledOnce();
    expect(button).toHaveAccessibleDescription("原有说明 补充说明");
    expect(Number.parseFloat(screen.getByRole("tooltip").style.top)).toBeLessThan(750);
    fireEvent.keyDown(button, { key: "Escape" });
    expect(button).toHaveAttribute("aria-describedby", "existing");
  });
});
