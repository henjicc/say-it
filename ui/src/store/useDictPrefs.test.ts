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
