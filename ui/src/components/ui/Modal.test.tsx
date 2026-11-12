import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Modal } from "./Modal";

describe("Modal body layout", () => {
  afterEach(cleanup);

  it("keeps normal dialogs scrollable by default", () => {
    render(<Modal open onClose={() => undefined} title="普通弹窗"><p>正文</p></Modal>);
    expect(screen.getByText("正文").parentElement).toHaveClass("overflow-y-auto");
    expect(screen.getByRole("heading").parentElement).toHaveClass("shrink-0");
  });

  it("retains custom body layout during exit instead of changing its scroll container", () => {
    const view = render(<Modal open onClose={() => undefined} bodyClassName="flex overflow-hidden"><p>固定布局</p></Modal>);
    const body = screen.getByText("固定布局").parentElement;
    expect(body).toHaveClass("flex", "overflow-hidden");
    expect(body).not.toHaveClass("overflow-y-auto");
    view.rerender(<Modal open={false} onClose={() => undefined}><p>已关闭</p></Modal>);
    expect(screen.getByText("固定布局").parentElement).toBe(body);
    expect(body).toHaveClass("overflow-hidden");
  });
});

describe("Modal focus", () => {
  afterEach(cleanup);

  it("moves focus into the dialog, keeps Tab inside, and returns focus on close", () => {
    const opener = document.createElement("button");
    document.body.append(opener);
    opener.focus();
    const dialog = (open: boolean) => (
      <Modal open={open} onClose={() => undefined}>
        <button>第一个</button>
        <button autoFocus>最后一个</button>
      </Modal>
    );
    const view = render(dialog(true));
    expect(screen.getByText("最后一个")).toHaveFocus();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Tab" });
    expect(screen.getByText("第一个")).toHaveFocus();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Tab", shiftKey: true });
    expect(screen.getByText("最后一个")).toHaveFocus();
    view.rerender(dialog(false));
    expect(opener).toHaveFocus();
    opener.remove();
  });
});

describe("Modal Escape", () => {
  afterEach(cleanup);

  it("closes only the topmost dialog", () => {
    const outer = vi.fn();
    const inner = vi.fn();
    render(
      <>
        <Modal open onClose={outer}>外层</Modal>
        <Modal open onClose={inner}>内层</Modal>
      </>,
    );
    fireEvent.keyDown(window, { key: "Escape" });
    expect(inner).toHaveBeenCalledTimes(1);
    expect(outer).not.toHaveBeenCalled();
  });
});
