// 本地快速处理引擎：整套本地处理就是一串有序的「正则 find→replace」规则。
// 语气词、口头禅、标点规范、句末补标点都是预置规则，用户可在规则表里增删改、调序。
// 本文件为纯逻辑（无 DOM / 无 worker 依赖），同时被主线程与 Web Worker 复用。

export interface LocalRule {
  id: string; // 内置规则用固定 id（便于升级合并）；自定义用 uuid
  enabled: boolean;
  name: string;
  pattern: string; // 正则源串（mode "find" 时不使用，运行时从 find 现算）
  flags: string; // 如 "g" / "gi" / "gm"
  replacement: string; // "" = 删除；regex 模式支持 $1 反向引用，find 模式为纯字面量
  builtin?: boolean; // 内置预置（可改可关、不可删；可一键恢复默认）
  note?: string;
  mode?: "regex" | "find"; // 默认 "regex"；"find" = 简单查找替换（非正则，自动处理中英文边界）
  find?: string; // mode "find" 时的查找文本（字面量，非正则）
}

const LEGACY_DEDUPE_PUNCT_PATTERN = "([，。！？、])\\1+";
const ADJACENT_PUNCT_PATTERN =
  "(?:…{2}|[，。！？、；：．,.!?;:])(?:[ \\t]*(?:…{2}|[，。！？、；：．,.!?;:]))+";
const ADJACENT_PUNCT_FLAGS = "g";
const BUILTIN_PUNCT_REPLACEMENT = "$1";
const BUILTIN_PUNCT_NOTE =
  "把没有正文夹在中间的连续标点合并成更自然的结果，如“，。”→“。”；会保留“……”以及“！？”这类常见合法组合。";

const ELLIPSIS_TOKENS = new Set(["……", "..."]);
const QUESTION_TOKENS = new Set(["？", "?"]);
const EXCLAMATION_TOKENS = new Set(["！", "!"]);
const PUNCT_PRIORITY = new Map<string, number>([
  ["，", 1],
  [",", 1],
  ["、", 1],
  ["；", 2],
  [";", 2],
  ["：", 2],
  [":", 2],
  ["。", 3],
  ["．", 3],
  [".", 3],
]);
const BUILTIN_PUNCT_RULE_SNAPSHOTS: Array<
  Pick<LocalRule, "name" | "pattern" | "flags" | "replacement" | "note" | "mode">
> = [
  {
    name: "合并连续重复标点",
    pattern: LEGACY_DEDUPE_PUNCT_PATTERN,
    flags: "g",
    replacement: "$1",
    note: undefined,
    mode: undefined,
  },
  {
    name: "合并连续标点",
    pattern: ADJACENT_PUNCT_PATTERN,
    flags: ADJACENT_PUNCT_FLAGS,
    replacement: BUILTIN_PUNCT_REPLACEMENT,
    note: BUILTIN_PUNCT_NOTE,
    mode: undefined,
  },
];

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
      id: "start-strip-punct",
      enabled: true,
      builtin: true,
      name: "删除句首标点",
      pattern: "^[ \\t]*[，。！？、；：．,.!?;:…]+[ \\t]*",
      flags: "g",
      replacement: "",
      note: "清理由句首语气词或口头禅被删除后遗留的逗号、句号等标点；保留引号和括号等有配对含义的符号。",
    },
    {
      id: "dedupe-punct",
      enabled: true,
      builtin: true,
      name: "合并连续标点",
      pattern: ADJACENT_PUNCT_PATTERN,
      flags: ADJACENT_PUNCT_FLAGS,
      replacement: BUILTIN_PUNCT_REPLACEMENT,
      note: BUILTIN_PUNCT_NOTE,
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
      id: "cjk-latin-space",
      enabled: true,
      builtin: true,
      name: "中英文之间自动加空格",
      pattern:
        "(?<=[\\u4e00-\\u9fa5])(?=[A-Za-z0-9])|(?<=[A-Za-z0-9])(?=[\\u4e00-\\u9fa5])",
      flags: "g",
      replacement: " ",
      note: "中文与英文字母/数字紧挨着时自动补一个空格，已有空格则不重复添加。",
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

const ASCII_WORD_CHAR_RE = /[A-Za-z0-9_]/;

function escapeRegExpLiteral(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// 替换文本按纯字面量处理时，需转义 $，避免被 String.replace 解释成 $1/$& 等特殊语法。
function escapeReplacementLiteral(s: string): string {
  return s.replace(/\$/g, "$$$$");
}

/**
 * 简单查找替换（find 模式）编译为正则源串：整体转义为字面量，
 * 仅在查找词首尾恰好是英文字母/数字/下划线时才加 \b，
 * 这样英文词紧贴中文（无空格）或被空格/标点围绕时都能命中，
 * 而查找词本身是中文时不会因误加 \b 导致匹配不到。
 */
export function buildFindPattern(find: string): string {
  const escaped = escapeRegExpLiteral(find);
  const startsWord = ASCII_WORD_CHAR_RE.test(find[0] ?? "");
  const endsWord = ASCII_WORD_CHAR_RE.test(find[find.length - 1] ?? "");
  return `${startsWord ? "\\b" : ""}${escaped}${endsWord ? "\\b" : ""}`;
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

function tokenizePunctuationCluster(cluster: string): string[] {
  const tokens: string[] = [];
  for (let i = 0; i < cluster.length; ) {
    if (cluster.startsWith("……", i)) {
      tokens.push("……");
      i += 2;
      continue;
    }
    if (cluster.startsWith("...", i)) {
      tokens.push("...");
      i += 3;
      continue;
    }
    const ch = cluster[i];
    if (ch === " " || ch === "\t") {
      i += 1;
      continue;
    }
    tokens.push(ch);
    i += 1;
  }
  return tokens;
}

function buildExpressiveSuffix(tokens: string[]): string {
  let sawQuestion = false;
  let sawExclamation = false;
  let suffix = "";

  for (const token of tokens) {
    if (QUESTION_TOKENS.has(token)) {
      if (!sawQuestion) suffix += token;
      sawQuestion = true;
      continue;
    }
    if (EXCLAMATION_TOKENS.has(token)) {
      if (!sawExclamation) suffix += token;
      sawExclamation = true;
    }
  }

  return suffix;
}

function pickStrongestPlainPunctuation(tokens: string[]): string {
  let winner = "";
  let bestRank = -1;

  for (const token of tokens) {
    const rank = PUNCT_PRIORITY.get(token) ?? -1;
    if (rank >= bestRank) {
      winner = token;
      bestRank = rank;
    }
  }

  return winner;
}

function normalizePunctuationCluster(cluster: string): string {
  const tokens = tokenizePunctuationCluster(cluster);
  if (tokens.length <= 1) return cluster.replace(/[ \t]+/g, "");

  if (tokens.some((token) => ELLIPSIS_TOKENS.has(token))) {
    let out = "";
    let sawQuestion = false;
    let sawExclamation = false;

    for (const token of tokens) {
      if (ELLIPSIS_TOKENS.has(token)) {
        out += token;
        continue;
      }
      if (QUESTION_TOKENS.has(token)) {
        if (!sawQuestion) out += token;
        sawQuestion = true;
        continue;
      }
      if (EXCLAMATION_TOKENS.has(token)) {
        if (!sawExclamation) out += token;
        sawExclamation = true;
      }
    }

    return out;
  }

  const expressiveSuffix = buildExpressiveSuffix(tokens);
  if (expressiveSuffix) return expressiveSuffix;

  return pickStrongestPlainPunctuation(tokens);
}

function isBuiltinPunctuationMergeRule(rule: LocalRule): boolean {
  return (
    rule.id === "dedupe-punct" &&
    rule.mode !== "find" &&
    rule.flags === ADJACENT_PUNCT_FLAGS &&
    rule.replacement === BUILTIN_PUNCT_REPLACEMENT &&
    (rule.pattern === LEGACY_DEDUPE_PUNCT_PATTERN || rule.pattern === ADJACENT_PUNCT_PATTERN)
  );
}

function shouldUpgradeBuiltinPunctuationMergeRule(rule: LocalRule): boolean {
  return (
    rule.id === "dedupe-punct" &&
    rule.builtin === true &&
    BUILTIN_PUNCT_RULE_SNAPSHOTS.some(
      (snapshot) =>
        rule.name === snapshot.name &&
        rule.pattern === snapshot.pattern &&
        rule.flags === snapshot.flags &&
        rule.replacement === snapshot.replacement &&
        rule.note === snapshot.note &&
        rule.mode === snapshot.mode,
    )
  );
}

function upgradeBuiltinRule(rule: LocalRule, preset: LocalRule): LocalRule {
  if (!shouldUpgradeBuiltinPunctuationMergeRule(rule)) return { ...rule };
  return {
    ...preset,
    enabled: rule.enabled,
  };
}

/**
 * 升级合并：保留用户已存的规则（含其开关 / 编辑 / 顺序），把本次版本新增的内置预置补到末尾。
 * 不覆盖用户对已知内置规则的改动。
 */
export function mergeLocalRules(stored: LocalRule[] | undefined): LocalRule[] {
  if (!Array.isArray(stored) || stored.length === 0) return defaultLocalRules();
  const presetRules = presets();
  const presetIds = new Set(presetRules.map((p) => p.id));
  const presetById = new Map(presetRules.map((p) => [p.id, p] as const));
  // 保留自定义规则与仍然存在的内置规则；丢弃已被产品移除的旧内置规则（如 end-period）。
  const merged = stored
    .filter((r) => !r.builtin || presetIds.has(r.id))
    .map((r) => {
      const preset = r.builtin ? presetById.get(r.id) : null;
      return preset ? upgradeBuiltinRule(r, preset) : { ...r };
    });
  const known = new Set(merged.map((r) => r.id));
  for (const preset of presetRules) {
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
    if (!r || !r.enabled) continue;
    let re: RegExp;
    let replacement: string;
    if (r.mode === "find") {
      if (!r.find) continue;
      try {
        re = new RegExp(buildFindPattern(r.find), r.flags?.includes("i") ? "gi" : "g");
      } catch {
        continue; // 非法正则：跳过
      }
      replacement = escapeReplacementLiteral(r.replacement ?? "");
    } else {
      if (!r.pattern) continue;
      try {
        re = new RegExp(r.pattern, r.flags || "g");
      } catch {
        continue; // 非法正则：跳过
      }
      replacement = r.replacement ?? "";
    }
    try {
      out = isBuiltinPunctuationMergeRule(r)
        ? out.replace(re, normalizePunctuationCluster)
        : out.replace(re, replacement);
    } catch {
      continue; // 替换异常（如非法 $ 引用）：跳过
    }
  }
  return out.trim();
}
