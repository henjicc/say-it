import { execFileSync } from "node:child_process";
import { existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const defaultAppPath = resolve("src-tauri/target/release/bundle/macos/说吧！.app");
const defaultDmgDirectory = resolve("src-tauri/target/release/bundle/dmg");

function latestDiskImage() {
  if (!existsSync(defaultDmgDirectory)) return null;
  return readdirSync(defaultDmgDirectory)
    .filter((name) => name.endsWith(".dmg"))
    .map((name) => resolve(defaultDmgDirectory, name))
    .sort((left, right) => statSync(right).mtimeMs - statSync(left).mtimeMs)[0] || null;
}

const requestedPath = resolve(
  process.argv[2] || (existsSync(defaultAppPath) ? defaultAppPath : latestDiskImage() || defaultAppPath),
);

if (!existsSync(requestedPath)) {
  throw new Error(`macOS 应用包或磁盘映像不存在：${requestedPath}`);
}

function command(commandPath, args) {
  return execFileSync(commandPath, args, { encoding: "utf8" }).trim();
}

function filesUnder(path) {
  const entries = [];
  for (const name of readdirSync(path)) {
    const child = resolve(path, name);
    const stat = lstatSync(child);
    if (stat.isDirectory()) entries.push(...filesUnder(child));
    else if (stat.isFile()) entries.push(child);
  }
  return entries;
}

function versionParts(value) {
  if (!/^\d+(?:\.\d+){0,2}$/.test(value)) {
    throw new Error(`无法解析 macOS 版本：${value}`);
  }
  return value.split(".").map(Number);
}

function compareVersions(left, right) {
  const a = versionParts(left);
  const b = versionParts(right);
  for (let index = 0; index < Math.max(a.length, b.length); index += 1) {
    const difference = (a[index] || 0) - (b[index] || 0);
    if (difference !== 0) return Math.sign(difference);
  }
  return 0;
}

function minimumVersions(binary) {
  const output = command("/usr/bin/otool", ["-l", binary]);
  const versions = [];
  const lines = output.split("\n");
  for (let index = 0; index < lines.length; index += 1) {
    const loadCommand = lines[index].trim();
    if (loadCommand !== "cmd LC_BUILD_VERSION" && loadCommand !== "cmd LC_VERSION_MIN_MACOSX") {
      continue;
    }
    const valueLabel = loadCommand === "cmd LC_BUILD_VERSION" ? "minos" : "version";
    for (let cursor = index + 1; cursor < Math.min(lines.length, index + 12); cursor += 1) {
      const match = lines[cursor].trim().match(new RegExp(`^${valueLabel}\\s+(\\S+)`));
      if (match) {
        versions.push(match[1]);
        break;
      }
    }
  }
  if (versions.length === 0) {
    throw new Error(`Mach-O 缺少最低系统版本：${binary}`);
  }
  return versions;
}

function validateApp(appPath) {
  const declaredMinimum = command("/usr/bin/plutil", [
    "-extract",
    "LSMinimumSystemVersion",
    "raw",
    "-o",
    "-",
    resolve(appPath, "Contents/Info.plist"),
  ]);
  const binaries = filesUnder(resolve(appPath, "Contents")).filter((path) =>
    command("/usr/bin/file", ["--brief", path]).includes("Mach-O"),
  );
  if (binaries.length === 0) {
    throw new Error(`macOS 应用包内没有 Mach-O：${appPath}`);
  }

  for (const binary of binaries) {
    for (const minimum of minimumVersions(binary)) {
      if (compareVersions(minimum, declaredMinimum) > 0) {
        throw new Error(
          `应用声明最低 macOS ${declaredMinimum}，但 ${binary} 要求 macOS ${minimum}`,
        );
      }
    }
  }

  console.log(`macOS 最低版本校验通过：声明 ${declaredMinimum}，检查 ${binaries.length} 个 Mach-O。`);
}

if (requestedPath.endsWith(".dmg")) {
  const temporaryDirectory = mkdtempSync(join(tmpdir(), "sayit-dmg-validation-"));
  const mountPath = resolve(temporaryDirectory, "mounted");
  mkdirSync(mountPath);
  let attached = false;
  try {
    execFileSync(
      "/usr/bin/hdiutil",
      ["attach", requestedPath, "-readonly", "-nobrowse", "-mountpoint", mountPath],
      { stdio: "ignore" },
    );
    attached = true;
    const apps = readdirSync(mountPath).filter((name) => name.endsWith(".app"));
    if (apps.length !== 1) {
      throw new Error(`macOS 磁盘映像应包含一个应用包，实际为 ${apps.length} 个：${requestedPath}`);
    }
    validateApp(resolve(mountPath, apps[0]));
  } finally {
    try {
      if (attached) {
        execFileSync("/usr/bin/hdiutil", ["detach", mountPath], { stdio: "ignore" });
      }
    } finally {
      rmSync(temporaryDirectory, { recursive: true, force: true });
    }
  }
} else {
  validateApp(requestedPath);
}
