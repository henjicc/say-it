// 本地快速处理引擎：整套本地处理就是一串有序的「正则 find→replace」规则。
// 语气词、口头禅、标点规范、句末补标点都是预置规则，用户可在规则表里增删改、调序。
// 本文件为纯逻辑（无 DOM / 无 worker 依赖），同时被主线程与 Web Worker 复用。

export interface LocalRule {
  id: string; // 内置规则用固定 id（便于升级合并）；自定义用 uuid
  enabled: boolean;
  name: string;
  pattern: string; // 正则源串
  flags: string; // 如 "g" / "gi" / "gm"
  replacement: string; // "" = 删除；支持 $1 反向引用
  builtin?: boolean; // 内置预置（可改可关、不可删；可一键恢复默认）
  note?: string;
}

// ---- 预置规则（保守默认：易误伤的项默认关闭）----
// 顺序即执行顺序：先清理语气词/口头禅，再规范空格标点，最后补句末标点。
function presets(): LocalRule[] {
  return [
    {
      id: "fillers-core",
      enabled: true,
      builtin: true,
      name: "去语气词（嗯/呃/唉/哎…）",
      pattern: "(?:嗯|呃|唉|呣|哎|噢|喔|唔|咦|嗯哼|呵呵)+",
      flags: "g",
      replacement: "",
      note: "这些字几乎不构成正常词语，任意位置出现都按语气词删除。可在此自行增减。",
    },
    {
      id: "fillers-bound",
      enabled: false,
      builtin: true,
      name: "去语气词（额/啊/哦，仅独立出现）",
      pattern: "(?<=^|[\\s，。！？、；：])(?:额|啊|哦|哎)+(?=$|[\\s，。！？、；：])",
      flags: "g",
      replacement: "",
      note: "额/啊/哦 常出现在正常词语里（如“额外”“好啊”），仅在被标点或空格包围、独立成词时才删，默认关闭。",
    },
    {
      id: "fillers-en",
      enabled: true,
      builtin: true,
      name: "去英文语气词（um/uh/er）",
      pattern: "\\b(?:um+|uh+|er+|hmm+)\\b",
      flags: "gi",
      replacement: "",
    },
    {
      id: "fillers-soft",
      enabled: false,
      builtin: true,
      name: "去口头禅（句首：那个/这个/然后/就是说）",
      pattern: "(?<=^|[。！？\\n])\\s*(?:那个|这个|然后|就是说|你知道吧|对吧)",
      flags: "g",
      replacement: "",
      note: "这些词常有正常用法（“那个人”），仅在句首出现时删除，默认关闭，请按需开启。",
    },
    {
      id: "dedupe-punct",
      enabled: true,
      builtin: true,
      name: "合并连续重复标点",
      pattern: "([，。！？、])\\1+",
      flags: "g",
      replacement: "$1",
    },
    {
      id: "punct-space",
      enabled: true,
      builtin: true,
      name: "清理中文标点旁空格",
      pattern: "[ \\t]*([，。！？、；：])[ \\t]*",
      flags: "g",
      replacement: "$1",
    },
    {
      id: "trim-spaces",
      enabled: true,
      builtin: true,
      name: "合并多余空格",
      pattern: " {2,}",
      flags: "g",
      replacement: " ",
    },
    {
      id: "collapse-repeat",
      enabled: false,
      builtin: true,
      name: "折叠叠字（同字 3 连及以上）",
      pattern: "([\\u4e00-\\u9fa5])\\1{2,}",
      flags: "g",
      replacement: "$1",
      note: "用于压掉口吃式重复，但“谢谢谢”这类也会被压，默认关闭。",
    },
    {
      id: "end-strip-punct",
      enabled: true,
      builtin: true,
      name: "删除句末标点",
      pattern: "[。．.！!？?、，,；;：:…]+\\s*$",
      flags: "g",
      replacement: "",
      note: "去掉结尾的句号 / 逗号等标点，适合注入到聊天框这类不想带句末标点的场景。",
    },
  ];
}

export function defaultLocalRules(): LocalRule[] {
  // 深拷贝，避免外部修改污染默认表。
  return presets().map((r) => ({ ...r }));
}

/** 校验单条规则的正则是否合法，返回 null 表示合法，否则返回错误信息。 */
export function validateRule(pattern: string, flags: string): string | null {
  if (!pattern) return null;
  try {
    new RegExp(pattern, flags || "g");
    return null;
  } catch (e) {
    return String((e as Error)?.message ?? e);
  }
}

/**
 * 升级合并：保留用户已存的规则（含其开关 / 编辑 / 顺序），把本次版本新增的内置预置补到末尾。
 * 不覆盖用户对已知内置规则的改动。
 */
export function mergeLocalRules(stored: LocalRule[] | undefined): LocalRule[] {
  if (!Array.isArray(stored) || stored.length === 0) return defaultLocalRules();
  const presetIds = new Set(presets().map((p) => p.id));
  // 保留自定义规则与仍然存在的内置规则；丢弃已被产品移除的旧内置规则（如 end-period）。
  const merged = stored
    .filter((r) => !r.builtin || presetIds.has(r.id))
    .map((r) => ({ ...r }));
  const known = new Set(merged.map((r) => r.id));
  for (const preset of presets()) {
    if (!known.has(preset.id)) merged.push({ ...preset });
  }
  return merged;
}

/**
 * 纯管道：按顺序对启用规则做 find→replace。
 * 单条规则非法 / 出错时跳过该条（不中断整条管道），最后统一去首尾空白。
 */
export function applyRulesPure(text: string, rules: LocalRule[]): string {
  let out = String(text ?? "");
  for (const r of rules) {
    if (!r || !r.enabled || !r.pattern) continue;
    let re: RegExp;
    try {
      re = new RegExp(r.pattern, r.flags || "g");
    } catch {
      continue; // 非法正则：跳过
    }
    try {
      out = out.replace(re, r.replacement ?? "");
    } catch {
      continue; // 替换异常（如非法 $ 引用）：跳过
    }
  }
  return out.trim();
}
