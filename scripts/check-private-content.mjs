// 拦截私人内容进仓库：媒体文件只能放在软件资源与测试素材目录；文本里不得出现用户主目录、
// 本机用户名、仓库所在目录、应用数据目录，以及本机配置 private-guard.local.json（不入库）里
// 列出的素材目录和词。具体的路径与名字都在运行时从本机读取，仓库里只有规则。
// --staged：pre-commit 钩子用，只查暂存区新增的文件和行；--all：CI 用，查全部已跟踪文件的
// 媒体白名单与通用主目录路径。
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

const MEDIA_EXT =
  /\.(png|jpe?g|gif|webp|bmp|ico|icns|svg|tiff?|heic|avif|psd|mp4|mov|webm|mkv|avi|m4v|mp3|wav|flac|ogg|m4a|aac|opus|pcm)$/i;
const MEDIA_ALLOWED = [
  "src-tauri/icons/",
  "src-tauri/resources/",
  "src-tauri/tests/fixtures/",
  "ui/public/",
  // README 用图逐个登记，换图时同步改这里。
  "docs/images/icon.png",
  "docs/images/01语音输入.jpg",
  "docs/images/02实时字幕.jpg",
  "docs/images/04设置.jpg",
];
// 原样引入的第三方代码与文档，以及本检查自身（测试里有故意构造的违规样例）。
const CONTENT_EXEMPT = [
  "src-tauri/vendor/",
  "docs/API/",
  "scripts/check-private-content.mjs",
  "scripts/check-private-content.test.mjs",
];
const LOCAL_CONFIG = "private-guard.local.json";
const APP_ID = "com.henjicc.sayit";
// 文档和测试里约定俗成的假用户名与系统公共目录。
const PLACEHOLDER_USERS = new Set(["user", "username", "example", "test", "shared", "public", "default"]);
const HOME_PATTERNS = [
  /(?:^|[^\w])[a-z]:\/+users\/+([^/\s<>%$'"`{}]+)/gi,
  /(?<![\w.-])\/(?:Users|home)\/([^/\s<>%$'"`{}]+)\//g,
];
const NAME_CHAR = /[\p{L}\p{N}_.-]/u;

export function isMediaAllowed(file) {
  return !MEDIA_EXT.test(file) || MEDIA_ALLOWED.some((prefix) => file === prefix || file.startsWith(prefix));
}

function normalizePath(value) {
  return value.replace(/\\+/g, "/").replace(/\/+$/, "").toLowerCase();
}

// 同一路径在文本里可能写成反斜杠、正斜杠、JSON 转义或 Git Bash 的 /d/... 形式。
export function pathNeedles(value) {
  const needle = normalizePath(value);
  const drive = /^([a-z]):\/(.*)$/.exec(needle);
  return drive ? [needle, `/${drive[1]}/${drive[2]}`] : [needle];
}

function containsPath(line, needle) {
  for (let at = line.indexOf(needle); at !== -1; at = line.indexOf(needle, at + 1)) {
    const next = line[at + needle.length];
    if (next === undefined || !NAME_CHAR.test(next)) return true;
  }
  return false;
}

/** 返回这一行命中的问题描述；needles 为 { label, paths, terms, user }。 */
export function findTextIssues(line, needles) {
  const normalized = line.replace(/\\+/g, "/");
  const lower = normalized.toLowerCase();
  const issues = [];
  for (const pattern of HOME_PATTERNS) {
    for (const match of normalized.matchAll(pattern)) {
      if (!PLACEHOLDER_USERS.has(match[1].toLowerCase())) {
        issues.push("用户主目录路径（改用 %USERPROFILE%、~ 或 <用户目录> 占位）");
        break;
      }
    }
  }
  for (const { label, paths } of needles.paths) {
    if (paths.some((needle) => containsPath(lower, needle))) issues.push(label);
  }
  if (needles.user && new RegExp(`/${escapeRegExp(needles.user)}(?![\\p{L}\\p{N}_.-])`, "iu").test(normalized)) {
    issues.push("本机用户名");
  }
  for (const term of needles.terms) {
    if (lower.includes(term)) issues.push("本机配置里登记的私人词");
  }
  return [...new Set(issues)];
}

function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function git(args) {
  return execFileSync("git", ["-c", "core.quotepath=false", ...args], { encoding: "utf8", maxBuffer: 1 << 28 });
}

function readJson(file) {
  return existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : undefined;
}

function appDataRoot() {
  const base =
    process.platform === "win32"
      ? process.env.LOCALAPPDATA && path.join(process.env.LOCALAPPDATA, APP_ID)
      : process.platform === "darwin"
        ? path.join(os.homedir(), "Library", "Application Support", APP_ID)
        : undefined;
  return base && readJson(path.join(base, "data-root.json"))?.root;
}

function localNeedles(repoRoot) {
  const config = readJson(path.join(repoRoot, LOCAL_CONFIG)) ?? {};
  const mainCheckout = path.dirname(git(["rev-parse", "--path-format=absolute", "--git-common-dir"]).trim());
  const entries = [
    ["用户主目录", os.homedir()],
    ["仓库所在目录（改用 <仓库目录> 占位）", repoRoot],
    ["仓库所在目录（改用 <仓库目录> 占位）", mainCheckout],
    ["应用数据目录", appDataRoot()],
    ...(config.privatePaths ?? []).map((value) => ["本机配置里登记的私人路径", value]),
  ];
  const user = os.userInfo().username;
  return {
    paths: entries.filter(([, value]) => value).map(([label, value]) => ({ label, paths: pathNeedles(value) })),
    terms: (config.privateTerms ?? []).map((term) => term.toLowerCase()),
    user: user.length >= 3 && !PLACEHOLDER_USERS.has(user.toLowerCase()) ? user : undefined,
  };
}

function isContentExempt(file) {
  return CONTENT_EXEMPT.some((prefix) => file.startsWith(prefix));
}

// 文件头只出现在 `diff --git` 与第一个 `@@` 之间；内容行本身以 `++ ` 开头时在 diff 里也是 `+++ `。
// 含引号、反斜杠等字符的文件名会被 git 加引号并转义，转义规则与 JSON 字符串一致。
export function parseAddedLines(diff) {
  const lines = [];
  let file;
  let inHeader = false;
  let lineNo = 0;
  for (const raw of diff.split("\n")) {
    if (raw.startsWith("diff --git ")) {
      inHeader = true;
      file = undefined;
    } else if (inHeader && raw.startsWith("+++ ")) {
      const name = raw.slice(4);
      file = name === "/dev/null" ? undefined : (name.startsWith('"') ? JSON.parse(name) : name).slice(2);
    } else if (raw.startsWith("@@")) {
      inHeader = false;
      lineNo = Number(/\+(\d+)/.exec(raw)[1]);
    } else if (!inHeader && raw.startsWith("+") && file) {
      lines.push({ file, lineNo: lineNo++, text: raw.slice(1) });
    }
  }
  return lines;
}

function stagedAddedLines() {
  return parseAddedLines(
    git(["diff", "--cached", "-U0", "--no-color", "--no-ext-diff", "--diff-filter=ACMR", "--src-prefix=a/", "--dst-prefix=b/"]),
  );
}

function trackedLines(files, repoRoot) {
  const lines = [];
  for (const file of files) {
    const bytes = readFileSync(path.join(repoRoot, file));
    if (bytes.subarray(0, 8000).includes(0)) continue;
    bytes.toString("utf8").split("\n").forEach((text, index) => lines.push({ file, lineNo: index + 1, text }));
  }
  return lines;
}

function main(mode) {
  const repoRoot = git(["rev-parse", "--show-toplevel"]).trim();
  const files =
    mode === "--staged"
      ? git(["diff", "--cached", "--name-only", "-z", "--diff-filter=ACMR"]).split("\0").filter(Boolean)
      : git(["ls-files", "-z"]).split("\0").filter(Boolean);
  const findings = files
    .filter((file) => !isMediaAllowed(file))
    .map((file) => `${file}\n    媒体文件只能放在：${MEDIA_ALLOWED.join("、")}`);

  // 本机目录与用户名只在作者机器上的钩子里有意义；CI 的是运行机的，只查通用规则。
  const needles = mode === "--staged" ? localNeedles(repoRoot) : { paths: [], terms: [], user: undefined };
  const lines =
    mode === "--staged"
      ? stagedAddedLines()
      : trackedLines(files.filter((file) => existsSync(path.join(repoRoot, file))), repoRoot);
  for (const { file, lineNo, text } of lines) {
    if (isContentExempt(file)) continue;
    for (const issue of findTextIssues(text, needles)) findings.push(`${file}:${lineNo}\n    ${issue}`);
  }

  if (findings.length === 0) return 0;
  console.error(`私人内容检查未通过（${findings.length} 处）：\n`);
  for (const finding of findings) console.error(`  ${finding}`);
  console.error("\n把文件移出仓库或改成占位后再提交；规则见 AGENTS.md「私人内容不进仓库」。");
  return 1;
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const mode = process.argv[2];
  if (mode !== "--staged" && mode !== "--all") {
    console.error("用法：node scripts/check-private-content.mjs --staged | --all");
    process.exit(2);
  }
  process.exit(main(mode));
}
