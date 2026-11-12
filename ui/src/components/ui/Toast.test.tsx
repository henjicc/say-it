import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { notify } from "@/store/useToastStore";
import { ToastHost } from "./Toast";

describe("ToastHost", () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { cleanup(); vi.useRealTimers(); });

  it("普通提示自动消失，错误留到用户关闭", () => {
    render(<ToastHost />);
    act(() => notify("已复制"));
    expect(screen.getByText("已复制")).toBeInTheDocument();
    act(() => { vi.advanceTimersByTime(3000); });
    expect(screen.queryByText("已复制")).not.toBeInTheDocument();

    act(() => notify("保存失败", "err"));
    act(() => { vi.advanceTimersByTime(60000); });
    expect(screen.getByRole("alert")).toHaveTextContent("保存失败");
    fireEvent.click(screen.getByRole("button", { name: "关闭提示" }));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
