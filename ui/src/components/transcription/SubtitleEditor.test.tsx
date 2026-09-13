import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { EditableCue } from "@/features/transcription/subtitles";

import { SubtitleEditor } from "./SubtitleEditor";
import { BASE_PX_PER_SEC } from "./subtitle-editor/constants";

function sampleCues(): EditableCue[] {
  return [
    { id: "c1", beginMs: 1000, endMs: 3000, text: "第一句" },
    { id: "c2", beginMs: 5000, endMs: 7000, text: "第二句" },
  ];
}

/**
 * 时间轴上那个可拖动的字幕块。
 *
 * 同一段文本会同时出现在时间轴的 span 和下方列表的 textarea 里，这里按
 * `cursor-grab` 认准时间轴那一个——它的父元素才是挂 onPointerDown 的块容器。
 */
function timelineBlock(text: string) {
  const block = screen
    .getAllByText(text)
    .map((node) => node.parentElement)
    .find((parent) => parent?.className.includes("cursor-grab"));
  if (!block) throw new Error(`找不到时间轴上的字幕块：${text}`);
  return block;
}

/**
 * jsdom 对 PointerEvent 的支持不完整，`fireEvent.pointerDown` 造出来的事件可能
 * 不带 `button` / `clientX`，会让 `onCuePointerDown` 在第一行就 early return。
 * 用 MouseEvent 造同名事件，属性齐全且 React 照样能收到。
 */
function firePointer(element: Element, type: string, clientX: number) {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, clientX });
  Object.defineProperty(event, "pointerId", { value: 1 });
  element.dispatchEvent(event);
}

beforeAll(() => {
  // jsdom 没有指针捕获 API，拖动开始时会用到。
  Element.prototype.setPointerCapture = vi.fn();
  Element.prototype.releasePointerCapture = vi.fn();
});

afterEach(cleanup);

describe("SubtitleEditor 时间轴拖动", () => {
  it("拖动写回的时间码必须是整数毫秒，否则导出 SRT 会被 Rust 侧 i64 拒收", () => {
    const onCuesChange = vi.fn();
    render(<SubtitleEditor mediaPath={null} cues={sampleCues()} onCuesChange={onCuesChange} />);

    const block = timelineBlock("第一句");
    // 100% 缩放下 pxPerSec = 60，拖 5px 即 83.333…ms——刻意选一个除不尽的位移。
    firePointer(block, "pointerdown", 100);
    firePointer(block, "pointermove", 105);

    expect((5 / BASE_PX_PER_SEC) * 1000).not.toBe(Math.round((5 / BASE_PX_PER_SEC) * 1000));
    expect(onCuesChange).toHaveBeenCalled();
    const next = onCuesChange.mock.calls.at(-1)?.[0] as EditableCue[];
    for (const cue of next) {
      expect(Number.isInteger(cue.beginMs)).toBe(true);
      expect(Number.isInteger(cue.endMs)).toBe(true);
    }
    // 位移确实生效了，不是因为拖动没触发才"恰好"是整数。
    expect(next[0].beginMs).not.toBe(1000);
  });
});

describe("SubtitleEditor 撤销快捷键", () => {
  it("输入框里的 Ctrl+Z 交还给浏览器原生撤销，不被编辑器劫持", () => {
    render(<SubtitleEditor mediaPath={null} cues={sampleCues()} onCuesChange={vi.fn()} />);

    const textarea = screen.getAllByRole("textbox").at(0);
    expect(textarea).toBeTruthy();

    const event = new KeyboardEvent("keydown", {
      key: "z",
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });
    textarea!.dispatchEvent(event);

    // 修复前这里是 true：监听挂在 window 捕获阶段且无条件 preventDefault，
    // 于是主窗口里任何输入框的原生撤销都被吃掉，并错误地改为撤销字幕改动。
    expect(event.defaultPrevented).toBe(false);
  });

  it("非输入框上的 Ctrl+Z 仍由编辑器接管", () => {
    render(<SubtitleEditor mediaPath={null} cues={sampleCues()} onCuesChange={vi.fn()} />);

    const event = new KeyboardEvent("keydown", {
      key: "z",
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });
    document.body.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(true);
  });
});
