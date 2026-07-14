#!/usr/bin/env python3
"""使用 Node.js 校验 ES 模块、工厂和方法形状，不访问真实供应商网络。"""

import argparse
import json
import shutil
import subprocess
import tempfile
from pathlib import Path

from validate_plugin import validate

RUNNER = r"""
import { pathToFileURL } from 'node:url';
const entry = process.argv[1];
const emitted = [];
const host = Object.freeze({
  http: Object.freeze({ request() { throw new Error('烟雾测试禁止真实网络'); } }),
  websocket: Object.freeze({ open() { throw new Error('烟雾测试禁止真实网络'); }, send() {}, close() {} }),
  base64: Object.freeze({ encode: value => Buffer.from(value).toString('base64'), decode: value => new Uint8Array(Buffer.from(value, 'base64')) }),
  crypto: Object.freeze({ randomBytes: size => new Uint8Array(size), sha256: () => '0'.repeat(64), hmacSha256: () => '0'.repeat(64) }),
  time: Object.freeze({ now: () => 0, sleep: () => {} }),
  storage: Object.freeze({ get: () => null, set: () => {}, delete: () => {} }),
  emit: event => emitted.push(event),
  log: () => {},
});
const module = await import(pathToFileURL(entry));
if (typeof module.default !== 'function') throw new Error('入口必须默认导出 createProvider(host)');
const provider = await module.default(host);
if (!provider || typeof provider !== 'object') throw new Error('createProvider 必须返回对象');
const allowed = new Set(['initialize','realtimeStart','realtimeAudio','realtimeFinish','realtimeStop','invoke','onHostEvent']);
for (const [name, value] of Object.entries(provider)) {
  if (!allowed.has(name)) throw new Error(`未知供应商方法：${name}`);
  if (typeof value !== 'function') throw new Error(`供应商成员必须是函数：${name}`);
}
if (provider.initialize) await provider.initialize({providerId:'smoke',config:{},session:null,permissions:[]});
if (provider.onHostEvent) await provider.onHostEvent({type:'smoke'});
console.log(JSON.stringify({methods:Object.keys(provider),emitted:emitted.length}));
"""


def main() -> int:
    parser = argparse.ArgumentParser(description="烟雾测试「说吧！」JavaScript 插件接口")
    parser.add_argument("plugin_dir", type=Path)
    args = parser.parse_args()
    root = args.plugin_dir.resolve()
    manifest = validate(root)
    with tempfile.TemporaryDirectory(prefix="sayit-smoke-", dir=root.parent) as temporary:
        test_root = Path(temporary)
        shutil.copytree(root / "connector", test_root / "connector")
        (test_root / "package.json").write_text('{"type":"module"}\n', encoding="utf-8")
        entry = test_root / "connector" / Path(manifest["runtime"]["entrypoint"]).relative_to("connector")
        process = subprocess.run(
            ["node", "--input-type=module", "-e", RUNNER, str(entry)],
            cwd=test_root,
            text=True,
            encoding="utf-8",
            capture_output=True,
            timeout=20,
            check=False,
        )
    if process.returncode != 0:
        raise SystemExit(f"SMOKE FAILED: {process.stderr[-1000:]}")
    result = json.loads(process.stdout.strip().splitlines()[-1])
    categories = {model["category"] for model in manifest["models"]}
    methods = set(result["methods"])
    if "realtime" in categories and not {"realtimeStart", "realtimeAudio", "realtimeFinish", "realtimeStop"}.issubset(methods):
        raise SystemExit("SMOKE FAILED: 实时模型缺少 realtimeStart/realtimeAudio/realtimeFinish/realtimeStop")
    if categories & {"file", "translation"} and "invoke" not in methods:
        raise SystemExit("SMOKE FAILED: 一次性模型缺少 invoke")
    print(f"SMOKE OK: {manifest['id']} ({', '.join(sorted(methods))})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
