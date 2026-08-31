import { spawnSync } from "node:child_process";

// Cargo tests compile the Tauri build script, but do not create installer
// assets. Clear bundle-only paths so a clean checkout can run tests before
// native sidecars and release libraries have been staged.
const testConfig = {
  bundle: {
    active: false,
    externalBin: [],
    resources: [],
    macOS: { frameworks: [] },
  },
};

const result = spawnSync(
  "cargo",
  ["test", "--manifest-path", "src-tauri/Cargo.toml", ...process.argv.slice(2)],
  {
    stdio: "inherit",
    env: {
      ...process.env,
      TAURI_CONFIG: JSON.stringify(testConfig),
    },
  },
);

if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
