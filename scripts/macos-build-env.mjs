import { existsSync } from "node:fs";
import { dirname } from "node:path";

/** 所有经 npm 脚本调用 cargo 的入口共用：最低系统版本与应用声明一致，并找到 Homebrew 的静态 Opus。 */
export function withMacBuildEnv(env) {
  if (process.platform !== "darwin") return env;
  const result = { ...env };
  // 与 tauri.macos.conf.json 的 minimumSystemVersion 一致，否则 cc 会按当前 SDK 抬高二进制的最低版本。
  result.MACOSX_DEPLOYMENT_TARGET ??= "15.5";
  // 没有 pkg-config 时 audiopus_sys 会转去源码编译 Opus，并因缺少 autoreconf 失败。
  if (!result.OPUS_LIB_DIR && !result.LIBOPUS_LIB_DIR) {
    const library = ["/opt/homebrew/lib/libopus.a", "/usr/local/lib/libopus.a"].find(existsSync);
    if (library) result.OPUS_LIB_DIR = dirname(library);
  }
  return result;
}
