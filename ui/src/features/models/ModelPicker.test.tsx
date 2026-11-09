import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ModelPicker, type ModelPickerOption } from "./ModelPicker";

const asrOptions: ModelPickerOption[] = [
  { value: "apple", label: "Apple 系统本地识别", providerId: "apple", providerLabel: "Apple", filterProviderId: "local", filterProviderLabel: "本地", mode: "realtime" },
  { value: "sensevoice", label: "SenseVoice Small 整句 INT8", providerId: "sensevoice", providerLabel: "SenseVoice", filterProviderId: "local", filterProviderLabel: "本地", mode: "realtime" },
  { value: "qwen-live", label: "Qwen Realtime", providerId: "bailian", providerLabel: "阿里云百炼", mode: "realtime" },
  { value: "qwen-file", label: "Qwen Flash", providerId: "bailian", providerLabel: "阿里云百炼", mode: "nonRealtime" },
  { value: "whisper", label: "Whisper", providerId: "groq", providerLabel: "Groq", mode: "nonRealtime" },
];

describe("ModelPicker", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("locates the current ASR model with a supplier rail, mode filter, and search at a fixed size", async () => {
    const onChange = vi.fn();
    const rect = (top: number, height: number, width = 400): DOMRect => ({
      x: 0,
      y: top,
      top,
      bottom: top + height,
      left: 0,
      right: width,
      width,
      height,
      toJSON: () => ({}),
    });
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function () {
      if (this.getAttribute("role") === "combobox") return rect(100, 44);
      return rect(0, 0);
    });
    vi.spyOn(HTMLElement.prototype, "offsetTop", "get").mockImplementation(function () {
      if (this.getAttribute("role") === "listbox") return 120;
      if (this.getAttribute("role") === "option" && this.getAttribute("aria-selected") === "true") return 460;
      if (this.getAttribute("aria-hidden") === "true") return { 本地: 120, 阿里云百炼: 220, Groq: 520 }[this.textContent ?? ""] ?? 0;
      return 0;
    });
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(function () {
      return this.getAttribute("role") === "option" ? 40 : 0;
    });
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockImplementation(function () {
      return this.getAttribute("role") === "listbox" ? 260 : 0;
    });
    vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(function () {
      return this.getAttribute("role") === "listbox" ? 600 : 0;
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
    expect(dialog).toHaveAttribute("data-state", "open");
    expect(dialog).toHaveAttribute("data-side", "bottom");
    expect(dialog.className).toContain("model-picker-panel");
    const search = within(dialog).getByRole("textbox", { name: "搜索语音识别模型" });
    await waitFor(() => expect(search).toHaveFocus());
    expect(within(dialog).getByRole("listbox", { name: "语音识别模型" }).scrollTop).toBe(230);
    expect(within(dialog).queryByText("当前选择")).not.toBeInTheDocument();
    expect(within(dialog).getByRole("option", { name: "Whisper" })).toHaveAttribute("aria-selected", "true");
    expect(within(dialog).getByRole("radio", { name: "全部" })).toHaveAttribute("aria-checked", "true");
    expect(dialog).toHaveStyle({ width: "640px", height: "440px" });
    const initialHeight = dialog.style.height;

    // 左栏按供应商定位：所有分组始终在列表里，点击只改变高亮位置。
    const rail = within(dialog).getByRole("navigation", { name: "按供应商定位" });
    expect(within(rail).getAllByRole("button").map((button) => button.getAttribute("aria-label")))
      .toEqual(["本地，2 个模型", "阿里云百炼，2 个模型", "Groq，1 个模型"]);
    expect(within(rail).getByRole("button", { name: "Groq，1 个模型" })).toHaveAttribute("aria-current", "location");
    const listbox = within(dialog).getByRole("listbox", { name: "语音识别模型" });
    fireEvent.click(within(rail).getByRole("button", { name: /阿里云百炼/u }));
    fireEvent.scroll(listbox);
    expect(within(rail).getByRole("button", { name: /阿里云百炼/u })).toHaveAttribute("aria-current", "location");
    // 末组太短、标题滚不到顶部：手动滚到底时高亮最后一组。
    listbox.scrollTop = 340;
    fireEvent.scroll(listbox);
    expect(within(rail).getByRole("button", { name: /Groq/u })).toHaveAttribute("aria-current", "location");
    expect(within(dialog).getByRole("option", { name: "Apple 系统本地识别" })).toBeInTheDocument();
    expect(within(dialog).getByRole("group", { name: "阿里云百炼" })).toBeInTheDocument();
    expect(within(within(dialog).getByRole("option", { name: "Qwen Flash" })).getByText("非实时")).toBeInTheDocument();
    expect(dialog.style.height).toBe(initialHeight);

    fireEvent.click(within(dialog).getByRole("radio", { name: "实时" }));
    expect(within(dialog).getByRole("option", { name: "Qwen Realtime" })).toBeInTheDocument();
    expect(within(dialog).queryByRole("option", { name: "Qwen Flash" })).not.toBeInTheDocument();
    expect(within(rail).queryByRole("button", { name: /Groq/u })).not.toBeInTheDocument();

    fireEvent.change(within(dialog).getByRole("textbox", { name: "搜索语音识别模型" }), {
      target: { value: "Realtime" },
    });
    fireEvent.click(within(dialog).getByRole("option", { name: "Qwen Realtime" }));
    expect(onChange).toHaveBeenCalledWith("qwen-live");
    expect(dialog).toHaveAttribute("data-state", "closed");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "选择语音识别模型" })).not.toBeInTheDocument());
  });

  it("uses the same searchable supplier panel for LLMs without ASR mode filters", async () => {
    render(
      <ModelPicker
        value="openai"
        options={[
          { value: "openai", label: "gpt-5", triggerLabel: "OpenAI · gpt-5", providerId: "openai", providerLabel: "OpenAI" },
          { value: "qwen", label: "qwen-max", triggerLabel: "阿里云百炼 · qwen-max", providerId: "bailian", providerLabel: "阿里云百炼", price: { text: "¥12 / ¥36", perMillionTokens: true } },
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
    expect(within(within(dialog).getByRole("option", { name: "qwen-max" })).getByText("¥12 / ¥36")).toBeInTheDocument();
    expect(within(within(dialog).getByRole("group", { name: "阿里云百炼" })).getByText("每百万 token 输入 / 输出")).toBeInTheDocument();
    expect(within(within(dialog).getByRole("group", { name: "OpenAI" })).queryByText("每百万 token 输入 / 输出")).not.toBeInTheDocument();
    fireEvent.change(within(dialog).getByRole("textbox", { name: "搜索智能模型" }), {
      target: { value: "百炼" },
    });
    expect(within(dialog).getByRole("option", { name: "qwen-max" })).toBeInTheDocument();
    expect(within(dialog).queryByRole("option", { name: "gpt-5" })).not.toBeInTheDocument();
  });
});
