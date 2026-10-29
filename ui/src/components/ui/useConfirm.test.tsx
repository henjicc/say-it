import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { useConfirm, type ConfirmOptions } from "./useConfirm";

let trigger: (options: ConfirmOptions) => Promise<boolean>;

function Harness() {
  const { confirm, dialog } = useConfirm();
  trigger = confirm;
  return <>{dialog}</>;
}

afterEach(cleanup);

const basic: ConfirmOptions = { title: "确认删除", message: "删除后不可恢复。" };

describe("useConfirm", () => {
  it("点确定时解析为 true，并展示标题与正文", async () => {
    render(<Harness />);
    let result: Promise<boolean>;
    act(() => {
      result = trigger(basic);
    });

    expect(screen.getByText("确认删除")).toBeInTheDocument();
    expect(screen.getByText("删除后不可恢复。")).toBeInTheDocument();

    await act(async () => {
      screen.getByRole("button", { name: "确定" }).click();
    });
    await expect(result!).resolves.toBe(true);
  });

  it("点取消时解析为 false", async () => {
    render(<Harness />);
    let result: Promise<boolean>;
    act(() => {
      result = trigger(basic);
    });
    await act(async () => {
      screen.getByRole("button", { name: "取消" }).click();
    });
    await expect(result!).resolves.toBe(false);
  });

  it("支持自定义按钮文案", async () => {
    render(<Harness />);
    let result: Promise<boolean>;
    act(() => {
      result = trigger({ ...basic, confirmLabel: "清空历史", cancelLabel: "再想想" });
    });
    expect(screen.getByRole("button", { name: "清空历史" })).toBeInTheDocument();
    await act(async () => {
      screen.getByRole("button", { name: "再想想" }).click();
    });
    await expect(result!).resolves.toBe(false);
  });

  /// 调用方几乎都写成 `if (!(await confirm(...))) return;`，一个永不 settle 的
  /// Promise 会让那条路径彻底卡住，所以被顶替的上一个必须按取消结算。
  it("新的确认请求会把上一个未决请求按取消结算，不留悬挂的 Promise", async () => {
    render(<Harness />);
    let first: Promise<boolean>;
    let second: Promise<boolean>;
    act(() => {
      first = trigger({ ...basic, title: "第一个" });
    });
    act(() => {
      second = trigger({ ...basic, title: "第二个" });
    });

    await expect(first!).resolves.toBe(false);

    await act(async () => {
      screen.getByRole("button", { name: "确定" }).click();
    });
    await expect(second!).resolves.toBe(true);
  });
});
