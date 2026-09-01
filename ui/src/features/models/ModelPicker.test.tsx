import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ModelPicker, type ModelPickerOption } from "./ModelPicker";

const asrOptions: ModelPickerOption[] = [
  { value: "apple", label: "Apple 系统本地识别（实时）", providerId: "apple", providerLabel: "Apple", filterProviderId: "local", filterProviderLabel: "本地", mode: "realtime" },
  { value: "sensevoice", label: "SenseVoice Small 整句 INT8（实时）", providerId: "sensevoice", providerLabel: "SenseVoice", filterProviderId: "local", filterProviderLabel: "本地", mode: "realtime" },
  { value: "qwen-live", label: "Qwen Realtime（实时）", providerId: "bailian", providerLabel: "阿里云百炼", mode: "realtime" },
  { value: "qwen-file", label: "Qwen Flash（非实时）", providerId: "bailian", providerLabel: "阿里云百炼", mode: "nonRealtime" },
  { value: "whisper", label: "Whisper（非实时）", providerId: "groq", providerLabel: "Groq", mode: "nonRealtime" },
];

describe("ModelPicker", () => {
  afterEach(cleanup);

  it("locates the current ASR model and combines local, mode, and search filters at a fixed size", async () => {
    const onChange = vi.fn();
    const scrollIntoView = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: scrollIntoView,
    });
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
    const search = within(dialog).getByRole("textbox", { name: "搜索语音识别模型" });
    await waitFor(() => expect(search).toHaveFocus());
    expect(scrollIntoView).toHaveBeenCalledWith({ block: "center" });
    expect(within(dialog).queryByText("当前选择")).not.toBeInTheDocument();
    expect(within(dialog).getByRole("option", { name: "Whisper（非实时）" })).toHaveAttribute("aria-selected", "true");
    expect(within(dialog).getByRole("button", { name: "供应商：全部" })).toHaveAttribute("aria-pressed", "true");
    expect(within(dialog).getByRole("button", { name: "识别方式：全部" })).toHaveAttribute("aria-pressed", "true");
    expect(dialog).toHaveStyle({ width: "520px", height: "440px" });
    const initialHeight = dialog.style.height;

    fireEvent.click(within(dialog).getByRole("button", { name: "供应商：本地" }));
    expect(within(dialog).getByRole("option", { name: "Apple 系统本地识别（实时）" })).toBeInTheDocument();
    expect(within(dialog).getByRole("option", { name: "SenseVoice Small 整句 INT8（实时）" })).toBeInTheDocument();
    expect(dialog.style.height).toBe(initialHeight);

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
    expect(dialog.className).toContain("dropdown-out");
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
