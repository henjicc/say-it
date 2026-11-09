import { beforeEach, describe, expect, it, vi } from "vitest";

const cmd = vi.fn();
vi.mock("@/lib/tauri", () => ({
  CMD: new Proxy({}, { get: (_target, key) => String(key) }),
  cmd: (...args: unknown[]) => cmd(...args),
}));
vi.mock("@/features/dictation/hotkeys", () => ({
  pruneShortcutProfileTemplates: vi.fn(async () => undefined),
}));

const { useDictPrefs } = await import("./useDictPrefs");

const initial = useDictPrefs.getState().prefs;

/** 写入排在微任务队列上，断言前先让出一轮。 */
const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  cmd.mockReset();
  cmd.mockResolvedValue(undefined);
  localStorage.clear();
  useDictPrefs.setState({ prefs: { ...initial, localRules: [] } });
});

const rule = (id: string) => ({
  id,
  enabled: true,
  name: id,
  pattern: id,
  flags: "g",
  replacement: "",
});

describe("听写设置的写入", () => {
  /// 本地规则的增删改序都基于整份数组重算。原实现用渲染时捕获的数组算负载并把
  /// promise 浮着：上一次保存还没返回就做下一次操作，后者会把前者整份覆盖掉。
  it("连续两次写入串行执行，后一次基于前一次的结果计算", async () => {
    const pending: Array<() => void> = [];
    cmd.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          pending.push(() => resolve());
        }),
    );

    const first = useDictPrefs
      .getState()
      .patch((current) => ({ localRules: [...current.localRules, rule("a")] }));
    const second = useDictPrefs
      .getState()
      .patch((current) => ({ localRules: [...current.localRules, rule("b")] }));

    await tick();
    expect(cmd).toHaveBeenCalledTimes(1);
    pending.shift()!();
    await first;
    await tick();
    expect(cmd).toHaveBeenCalledTimes(2);
    pending.shift()!();
    await second;

    expect(useDictPrefs.getState().prefs.localRules.map((item) => item.id)).toEqual(["a", "b"]);
  });

  /// 后端 validate_rules 会拒绝写不完整的正则（例如刚敲到 `([`）。调用方必须拿得到
  /// 这个 rejection，否则界面只会无声回滚。
  it("后端拒绝时 patch 会 reject，本地状态保持不变", async () => {
    useDictPrefs.setState({ prefs: { ...initial, localRules: [rule("keep")] } });
    cmd.mockRejectedValueOnce(new Error("规则 bad 不兼容"));

    await expect(
      useDictPrefs.getState().patch({ localRules: [rule("keep"), rule("([")] }),
    ).rejects.toThrow(/不兼容/);

    expect(useDictPrefs.getState().prefs.localRules.map((item) => item.id)).toEqual(["keep"]);
  });

  /// 拖动滑块每帧都会 patch；界面要立刻跟手，在途期间的改动合成一次保存，而不是排队整份落盘几十次。
  it("界面立即显示新值，保存在途期间的多次改动合并为一次保存", async () => {
    const pending: Array<() => void> = [];
    cmd.mockImplementation(() => new Promise<void>((resolve) => pending.push(resolve)));

    const writes = [0.2, 0.4, 0.6, 0.8].map((denoiseStrength) =>
      useDictPrefs.getState().patch({ denoiseStrength }),
    );
    expect(useDictPrefs.getState().prefs.denoiseStrength).toBe(0.8);

    await tick();
    expect(cmd).toHaveBeenCalledTimes(1);
    pending.shift()!();
    await tick();
    expect(cmd).toHaveBeenCalledTimes(2);
    expect(cmd.mock.calls[1][1].value.denoiseStrength).toBe(0.8);
    pending.shift()!();
    await Promise.all(writes);
    expect(cmd).toHaveBeenCalledTimes(2);
  });

  it("一次失败不会堵死后续写入", async () => {
    cmd.mockRejectedValueOnce(new Error("磁盘满了"));
    await expect(useDictPrefs.getState().patch({ localRules: [rule("x")] })).rejects.toThrow(
      /磁盘满了/,
    );

    cmd.mockResolvedValue(undefined);
    await useDictPrefs.getState().patch({ localRules: [rule("y")] });
    expect(useDictPrefs.getState().prefs.localRules.map((item) => item.id)).toEqual(["y"]);
  });
});

describe("内置模板升级", () => {
  it("升级目录时保留用户改过的场景感知润色，新装默认用带使用场景的版本", async () => {
    const { APP_SCENE_PLACEHOLDER, defaultSmartTextTemplates, hydrateDictPrefs } = await import("./useDictPrefs");
    const builtin = defaultSmartTextTemplates().find((template) => template.id === "context-aware-polish");
    expect(builtin?.prompt).toContain(APP_SCENE_PLACEHOLDER);

    const customized = { id: "context-aware-polish", name: "场景感知润色", prompt: "我自己的提示词 {{text}}" };
    hydrateDictPrefs({ smartTemplateCatalogVersion: 5, smartTemplates: [customized], smartTemplateId: "context-aware-polish" });
    expect(useDictPrefs.getState().prefs.smartTemplates.find((template) => template.id === "context-aware-polish")?.prompt)
      .toBe(customized.prompt);
  });

  it("内置本地规则升级后要求回写到后端", async () => {
    const { hydrateDictPrefs } = await import("./useDictPrefs");
    hydrateDictPrefs({ localRules: [] });
    const current = { ...useDictPrefs.getState().prefs } as unknown as Record<string, unknown>;
    expect(hydrateDictPrefs(current)).toBe(false);
    const legacy = useDictPrefs.getState().prefs.localRules.filter((rule) => rule.id !== "dedupe-stutter");
    expect(hydrateDictPrefs({ ...current, localRules: legacy })).toBe(true);
  });

  it("目录升级不改动用户主动选中的其他模板", async () => {
    const { defaultSmartTextTemplates, hydrateDictPrefs } = await import("./useDictPrefs");
    hydrateDictPrefs({ smartTemplateCatalogVersion: 5, smartTemplates: defaultSmartTextTemplates(), smartTemplateId: "polish" });
    expect(useDictPrefs.getState().prefs.smartTemplateId).toBe("polish");
  });
});
