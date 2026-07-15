import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

if (process.platform !== "win32") {
  console.log("context-probe: non-Windows platform, skipped.");
  process.exit(0);
}

const script = fileURLToPath(new URL("./构建上下文探针.ps1", import.meta.url));
const result = spawnSync(
  "pwsh",
  ["-NoLogo", "-NoProfile", "-NonInteractive", "-File", script],
  { stdio: "inherit" },
);
if (result.error) throw result.error;
process.exit(result.status ?? 1);
