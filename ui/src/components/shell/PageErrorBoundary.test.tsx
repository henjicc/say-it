import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PageErrorBoundary } from "./PageErrorBoundary";

function Broken(): never {
  throw new Error("读取失败的字段");
}

describe("PageErrorBoundary", () => {
  afterEach(cleanup);

  it("页面渲染出错时显示错误，而不是整个界面消失", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(<PageErrorBoundary><Broken /></PageErrorBoundary>);
    expect(screen.getByRole("alert")).toHaveTextContent("读取失败的字段");
    expect(screen.getByRole("button", { name: "重新加载" })).toBeInTheDocument();
  });
});
