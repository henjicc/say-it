import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
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
