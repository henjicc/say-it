import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  normalizeStoredParams,
  parseSpeakerCount,
  useTranscriptionParamsSync,
  type TranscriptionParamsSyncOptions,
} from "./paramsSync";
import { DEFAULT_TRANSCRIPTION_PARAMS, type TranscriptionParams } from "@/store/useTranscriptionStore";

const DEBOUNCE = 10;

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

/**
 * 模拟真实的一轮：界面参数是外部状态，保存成功后「磁盘」会换成一份新对象，
 * 那份新对象的身份变化会驱动回填 effect ——这正是缺陷发生的地方。
 */
function setup(initialStored: unknown = {}) {
  let stored: unknown = initialStored;
  let params: TranscriptionParams = normalizeStoredParams(initialStored);
  const replaceParams = vi.fn((next: TranscriptionParams) => {
    params = next;
  });
  const messages: string[] = [];
  let resolveSave: (() => void) | null = null;
  const save = vi.fn(
    (next: TranscriptionParams) =>
      new Promise<void>((resolve) => {
        resolveSave = () => {
          // 写盘成功：provider store 整体换新，stored 拿到一个全新的对象。
          stored = { ...next };
          resolve();
        };
      }),
  );

  const view = renderHook(
    (props: Partial<TranscriptionParamsSyncOptions>) =>
      useTranscriptionParamsSync({
        stored,
        params,
        enabled: true,
        replaceParams,
        save,
        onMessage: (text) => messages.push(text),
        debounceMs: DEBOUNCE,
        ...props,
      }),
    { initialProps: {} },
  );

  return {
    view,
    save,
    replaceParams,
    messages,
    get params() {
      return params;
    },
    edit(patch: Partial<TranscriptionParams>) {
      params = { ...params, ...patch };
      view.rerender({});
    },
    flushSave() {
      resolveSave?.();
      resolveSave = null;
    },
  };
}

describe("useTranscriptionParamsSync", () => {
  it("界面改动防抖后写盘一次", async () => {
    const harness = setup();
    harness.edit({ diarizationEnabled: true });

    await act(async () => {
      vi.advanceTimersByTime(DEBOUNCE);
    });

    expect(harness.save).toHaveBeenCalledTimes(1);
    expect(harness.save.mock.calls[0][0].diarizationEnabled).toBe(true);
  });

  /// 保存成功会让 provider store 换一份新的 profiles，回填 effect 因此每次保存都会
  /// 重跑一次。原实现在那里无条件 replaceParams，于是往返期间用户做的第二次修改被
  /// 回滚，而 params 被改回旧值又会 clearTimeout 掉待触发的那次保存——改动既不生效
  /// 也不入库，界面还显示「识别参数已保存」。
  it("保存往返期间的第二次修改不被回滚，并且照样入库", async () => {
    const harness = setup();

    harness.edit({ diarizationEnabled: true });
    await act(async () => {
      vi.advanceTimersByTime(DEBOUNCE);
    });
    expect(harness.save).toHaveBeenCalledTimes(1);

    // 第一次写盘还没返回，用户又改了一处。
    harness.edit({ speakerCount: 3 });

    // 写盘返回，stored 换成第一次保存的那份。
    await act(async () => {
      harness.flushSave();
    });
    harness.view.rerender({});

    expect(harness.params.speakerCount).toBe(3);
    expect(harness.params.diarizationEnabled).toBe(true);

    await act(async () => {
      vi.advanceTimersByTime(DEBOUNCE);
    });

    expect(harness.save).toHaveBeenCalledTimes(2);
    expect(harness.save.mock.calls[1][0].speakerCount).toBe(3);
  });

  it("供应商配置发生真正的外部变化时仍然回填界面", () => {
    const harness = setup();
    act(() => {
      harness.view.rerender({ stored: { diarizationEnabled: true, speakerCount: 5 } });
    });

    expect(harness.replaceParams).toHaveBeenCalledWith(
      expect.objectContaining({ diarizationEnabled: true, speakerCount: 5 }),
    );
  });

  it("写盘失败时报错，并且下一次改动仍会重试", async () => {
    let stored: unknown = {};
    let params = normalizeStoredParams(stored);
    const messages: string[] = [];
    const save = vi.fn().mockRejectedValue(new Error("磁盘满了"));
    const view = renderHook(() =>
      useTranscriptionParamsSync({
        stored,
        params,
        enabled: true,
        replaceParams: (next) => {
          params = next;
        },
        save,
        onMessage: (text) => messages.push(text),
        debounceMs: DEBOUNCE,
      }),
    );

    params = { ...params, diarizationEnabled: true };
    view.rerender();
    await act(async () => {
      vi.advanceTimersByTime(DEBOUNCE);
    });

    expect(messages.at(-1)).toMatch(/识别参数保存失败/);

    // 失败后必须回退登记的 key，否则同一份参数再也不会被重试。
    params = { ...params };
    view.rerender();
    await act(async () => {
      vi.advanceTimersByTime(DEBOUNCE);
    });
    expect(save).toHaveBeenCalledTimes(2);
  });

  it("没有可用供应商时不写盘", async () => {
    const harness = setup();
    harness.edit({ diarizationEnabled: true });
    harness.view.rerender({ enabled: false });

    await act(async () => {
      vi.advanceTimersByTime(DEBOUNCE);
    });

    expect(harness.save).not.toHaveBeenCalled();
  });
});

describe("normalizeStoredParams", () => {
  it("非法输入收敛成默认值", () => {
    expect(normalizeStoredParams(null)).toEqual(DEFAULT_TRANSCRIPTION_PARAMS);
    expect(normalizeStoredParams({ speakerCount: -1 }).speakerCount).toBeNull();
    expect(normalizeStoredParams({ languageHints: ["zh", 5] }).languageHints).toEqual(["zh"]);
  });
});

describe("parseSpeakerCount", () => {
  /// 后端 speaker_count 是 Option<u32>，小数会让 serde 报 invalid type: floating point，
  /// **整个** transcription_start 的参数反序列化失败——不只是说话人分离失效。而这个值
  /// 还会被持久化进供应商配置，重启也不自愈。仓库其它数字字段一律走 NumberInput 的
  /// Number.parseInt，这里因为要支持「留空」才没用它，解析口径必须对齐。
  it("只产出整数，绝不把小数交给后端", () => {
    expect(parseSpeakerCount("3.7")).toBe(3);
    expect(parseSpeakerCount("2.0")).toBe(2);
    expect(parseSpeakerCount("  5  ")).toBe(5);
    expect(Number.isInteger(parseSpeakerCount("9.99"))).toBe(true);
  });

  it("留空表示自动判断", () => {
    expect(parseSpeakerCount("")).toBeNull();
    expect(parseSpeakerCount("   ")).toBeNull();
  });

  it("非法与非正数一律回落到自动", () => {
    expect(parseSpeakerCount("abc")).toBeNull();
    expect(parseSpeakerCount("0")).toBeNull();
    expect(parseSpeakerCount("-3")).toBeNull();
  });

  it("历史上已经写进配置的小数在读回时自愈", () => {
    expect(normalizeStoredParams({ speakerCount: 3.7 }).speakerCount).toBe(3);
    expect(normalizeStoredParams({ speakerCount: 0.4 }).speakerCount).toBeNull();
    expect(normalizeStoredParams({ speakerCount: 4 }).speakerCount).toBe(4);
  });
});
