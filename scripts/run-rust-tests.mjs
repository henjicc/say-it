import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, rmSync } from "node:fs";
import path from "node:path";

const sherpaLinkLibraryPattern = /^(?:lib)?sherpa-onnx-c-api\.(?:a|dylib|lib|so)$/i;

function containsSherpaLinkLibrary(root) {
  if (!existsSync(root)) return false;
  const pending = [root];
  while (pending.length > 0) {
    const current = pending.pop();
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      if (entry.isDirectory()) {
        pending.push(path.join(current, entry.name));
      } else if (sherpaLinkLibraryPattern.test(entry.name)) {
        return true;
      }
    }
  }
  return false;
}

function removeMatchingEntries(root, pattern) {
  if (!existsSync(root)) return;
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    if (pattern.test(entry.name)) {
      rmSync(path.join(root, entry.name), { recursive: true, force: true });
    }
  }
}

function repairIncompleteSherpaCache() {
  if (process.env.SHERPA_ONNX_LIB_DIR) return;
  const targetRoot = path.resolve(process.env.CARGO_TARGET_DIR ?? "src-tauri/target");
  const prebuiltRoot = path.join(targetRoot, "sherpa-onnx-prebuilt");
  if (containsSherpaLinkLibrary(prebuiltRoot)) return;

  // Rust caches can retain a completed build-script fingerprint while omitting
  // downloaded native libraries. Remove only this crate's generated entries so
  // Cargo reruns its build script and restores the matching platform library.
  rmSync(prebuiltRoot, { recursive: true, force: true });
  for (const profile of ["debug", "release"]) {
    const profileRoot = path.join(targetRoot, profile);
    removeMatchingEntries(path.join(profileRoot, "build"), /^sherpa-onnx-sys-/);
    removeMatchingEntries(path.join(profileRoot, ".fingerprint"), /^sherpa-onnx-sys-/);
    removeMatchingEntries(path.join(profileRoot, "deps"), /sherpa_onnx_sys/);
  }
}

repairIncompleteSherpaCache();

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
