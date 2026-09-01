import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ModelPicker, type ModelPickerOption } from "./ModelPicker";

const asrOptions: ModelPickerOption[] = [
  { value: "apple", label: "Apple 系统本地识别（实时）", providerId: "apple", providerLabel: "Apple", mode: "realtime" },
  { value: "qwen-live", label: "Qwen Realtime（实时）", providerId: "bailian", providerLabel: "阿里云百炼", mode: "realtime" },
  { value: "qwen-file", label: "Qwen Flash（非实时）", providerId: "bailian", providerLabel: "阿里云百炼", mode: "nonRealtime" },
  { value: "whisper", label: "Whisper（非实时）", providerId: "groq", providerLabel: "Groq", mode: "nonRealtime" },
];

describe("ModelPicker", () => {
  afterEach(cleanup);

  it("keeps the current ASR model visible and combines provider, mode, and search filters", async () => {
    const onChange = vi.fn();
    render(
      <ModelPicker
        value="whisper"
        options={asrOptions}
        aria-label="识别模型"
        panelLabel="选择语音识别模型"
        onChange={onChange}
      />,
    );

    fireEvent.click(screen.getByRole("combobox", { name: "识别模型" }));
    const dialog = await screen.findByRole("dialog", { name: "选择语音识别模型" });
    expect(within(dialog).getByText("当前选择")).toBeInTheDocument();
    expect(within(dialog).getAllByText("Whisper（非实时）").length).toBeGreaterThan(0);

    fireEvent.click(within(dialog).getByRole("button", { name: "供应商：阿里云百炼" }));
    expect(within(dialog).getByRole("option", { name: "Qwen Realtime（实时）" })).toBeInTheDocument();
    expect(within(dialog).getByRole("option", { name: "Qwen Flash（非实时）" })).toBeInTheDocument();
    expect(within(dialog).queryByRole("option", { name: "Whisper（非实时）" })).not.toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole("button", { name: "识别方式：实时" }));
    expect(within(dialog).getByRole("option", { name: "Qwen Realtime（实时）" })).toBeInTheDocument();
    expect(within(dialog).queryByRole("option", { name: "Qwen Flash（非实时）" })).not.toBeInTheDocument();

    fireEvent.change(within(dialog).getByRole("textbox", { name: "搜索语音识别模型" }), {
      target: { value: "Realtime" },
    });
    fireEvent.click(within(dialog).getByRole("option", { name: "Qwen Realtime（实时）" }));
    expect(onChange).toHaveBeenCalledWith("qwen-live");
  });

  it("uses the same searchable supplier panel for LLMs without ASR mode filters", async () => {
    render(
      <ModelPicker
        value="openai"
        options={[
          { value: "openai", label: "gpt-5", triggerLabel: "OpenAI · gpt-5", providerId: "openai", providerLabel: "OpenAI" },
          { value: "qwen", label: "qwen-max", triggerLabel: "阿里云百炼 · qwen-max", providerId: "bailian", providerLabel: "阿里云百炼" },
        ]}
        aria-label="智能模型"
        panelLabel="选择智能模型"
        onChange={() => undefined}
      />,
    );

    expect(screen.getByRole("combobox", { name: "智能模型" })).toHaveTextContent("OpenAI · gpt-5");
    fireEvent.click(screen.getByRole("combobox", { name: "智能模型" }));
    const dialog = await screen.findByRole("dialog", { name: "选择智能模型" });
    expect(within(dialog).queryByRole("group", { name: "按识别方式筛选" })).not.toBeInTheDocument();
    fireEvent.change(within(dialog).getByRole("textbox", { name: "搜索智能模型" }), {
      target: { value: "百炼" },
    });
    expect(within(dialog).getByRole("option", { name: "qwen-max" })).toBeInTheDocument();
    expect(within(dialog).queryByRole("option", { name: "gpt-5" })).not.toBeInTheDocument();
  });
});
