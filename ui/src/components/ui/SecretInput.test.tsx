import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";

import { SecretInput } from "./SecretInput";

afterEach(cleanup);

function Harness({
  hasStoredValue,
  onRevealStored,
}: {
  hasStoredValue: boolean;
  onRevealStored?: () => Promise<string>;
}) {
  const [draft, setDraft] = useState("");
  return (
    <>
      <SecretInput
        aria-label="密码"
        draftValue={draft}
        hasStoredValue={hasStoredValue}
        onDraftChange={setDraft}
        onRevealStored={onRevealStored}
        placeholder="未启用认证可留空"
      />
      <output data-testid="draft">{draft}</output>
      <button type="button">别处</button>
    </>
  );
}

const field = () => screen.getByLabelText("密码") as HTMLInputElement;

describe("SecretInput", () => {
  /// 铁律：掩码只能是展示状态，绝不能进入 input value——否则它会随表单一起被
  /// 当成真密码提交，把用户已保存的凭据换成一串圆点。
  it("已保存的密钥用 placeholder 呈现掩码，input value 始终为空", () => {
    render(<Harness hasStoredValue />);
    expect(field().value).toBe("");
    expect(field().placeholder).toMatch(/^•+$/);
  });

  it("没有已保存值时展示正常占位文案", () => {
    render(<Harness hasStoredValue={false} />);
    expect(field().placeholder).toBe("未启用认证可留空");
    expect(field().value).toBe("");
  });

  it("输入的草稿原样上报，并且默认是密码框", () => {
    render(<Harness hasStoredValue />);
    fireEvent.change(field(), { target: { value: "新密码" } });
    expect(screen.getByTestId("draft")).toHaveTextContent("新密码");
    expect(field().type).toBe("password");
  });

  it("没有 onRevealStored 时，已保存的值不可显示", () => {
    render(<Harness hasStoredValue />);
    expect(screen.getByRole("button", { name: "显示密钥" })).toBeDisabled();
  });

  it("提供 onRevealStored 时按需读取明文，且不写进草稿", async () => {
    const reveal = vi.fn().mockResolvedValue("真密码");
    render(<Harness hasStoredValue onRevealStored={reveal} />);

    await act(async () => {
      screen.getByRole("button", { name: "显示密钥" }).click();
    });

    await waitFor(() => expect(field().value).toBe("真密码"));
    expect(field().type).toBe("text");
    // 明文只是展示，不能污染将要保存的草稿。
    expect(screen.getByTestId("draft")).toHaveTextContent("");
    expect(reveal).toHaveBeenCalledTimes(1);
  });

  it("再次点击隐藏后明文立刻消失", async () => {
    const reveal = vi.fn().mockResolvedValue("真密码");
    render(<Harness hasStoredValue onRevealStored={reveal} />);
    await act(async () => {
      screen.getByRole("button", { name: "显示密钥" }).click();
    });
    await waitFor(() => expect(field().value).toBe("真密码"));

    await act(async () => {
      screen.getByRole("button", { name: "隐藏密钥" }).click();
    });
    expect(field().value).toBe("");
  });

  it("失焦后不再残留已读取的明文", async () => {
    const reveal = vi.fn().mockResolvedValue("真密码");
    render(<Harness hasStoredValue onRevealStored={reveal} />);
    await act(async () => {
      screen.getByRole("button", { name: "显示密钥" }).click();
    });
    await waitFor(() => expect(field().value).toBe("真密码"));

    fireEvent.blur(field());
    expect(field().value).toBe("");
  });

  it("读取失败返回空串时保持隐藏，不显示任何内容", async () => {
    const reveal = vi.fn().mockResolvedValue("");
    render(<Harness hasStoredValue onRevealStored={reveal} />);
    await act(async () => {
      screen.getByRole("button", { name: "显示密钥" }).click();
    });
    expect(field().value).toBe("");
    expect(field().type).toBe("password");
  });
});
