import { describe, expect, it } from "vitest";
import { applyRulesPure, defaultLocalRules, mergeLocalRules, type LocalRule } from "./localRulesEngine";

const LEGACY_FILLERS_CORE: LocalRule = {
  id: "fillers-core",
  enabled: false,
  builtin: true,
  name: "去语气词（嗯/呃/唉/哎…）",
  pattern: "(?:嗯|呃|唉|呣|哎|噢|喔|唔|咦|嗯哼|呵呵)+",
  flags: "g",
  replacement: "",
  note: "这些字几乎不构成正常词语，任意位置出现都按语气词删除。可在此自行增减。",
};

describe("本地规则升级合并", () => {
  it("没改过的内置规则升级到当前预置，用户改过的开关和内容保留", () => {
    const presets = defaultLocalRules();
    const edited = { ...presets.find((r) => r.id === "fillers-soft")!, pattern: "我的写法", enabled: false };
    const merged = mergeLocalRules([LEGACY_FILLERS_CORE, edited]);

    const core = merged.find((r) => r.id === "fillers-core")!;
    expect(core.pattern).toBe(presets.find((r) => r.id === "fillers-core")!.pattern);
    expect(core.enabled).toBe(false);
    expect(merged.find((r) => r.id === "fillers-soft")).toEqual(edited);
  });

  it("新增的内置规则插在预置顺序里的位置，不追加到删除句末标点之后", () => {
    const stored = defaultLocalRules().filter((r) => r.id !== "dedupe-stutter");
    const ids = mergeLocalRules(stored).map((r) => r.id);
    expect(ids.indexOf("dedupe-stutter")).toBe(ids.indexOf("fillers-en") + 1);
  });
});

describe("默认规则", () => {
  it("清掉语气词、句首口头禅和口吃重复，保留叹词、实义的“这个”和问号", () => {
    const text = "嗯，然后我我觉得可以。嗯，然后的话哎呀太宽了。这个可不行，你觉得呢？";
    expect(applyRulesPure(text, defaultLocalRules())).toBe("我觉得可以。哎呀太宽了。这个可不行，你觉得呢？");
  });

  it("不误删有意思的追问、短回复和隔着标点的分句", () => {
    const rules = defaultLocalRules();
    expect(applyRulesPure("他走了。然后呢？", rules)).toBe("他走了。然后呢？");
    expect(applyRulesPure("然后呢", rules)).toBe("然后呢");
    expect(applyRulesPure("哦。", rules)).toBe("哦");
    expect(applyRulesPure("你相信我，我们能赢。", rules)).toBe("你相信我，我们能赢");
    expect(applyRulesPure("然后然后我觉得可以", rules)).toBe("我觉得可以");
  });
});
