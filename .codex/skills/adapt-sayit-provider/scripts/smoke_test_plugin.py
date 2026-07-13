#!/usr/bin/env python3
import argparse
import json
import subprocess
import uuid
from pathlib import Path

from validate_plugin import validate


def exchange(root: Path, manifest: dict, messages: list[dict]) -> list[dict]:
    runtime = manifest["runtime"]
    process = subprocess.Popen(
        [str(root / runtime["entrypoint"]), *runtime.get("args", [])],
        cwd=root,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    payload = "".join(json.dumps(message, ensure_ascii=False, separators=(",", ":")) + "\n" for message in messages)
    stdout, stderr = process.communicate(payload, timeout=20)
    if process.returncode not in {0, None}:
        raise RuntimeError(f"connector exited {process.returncode}: {stderr[-500:]}")
    return [json.loads(line) for line in stdout.splitlines() if line.strip()]


def main() -> int:
    parser = argparse.ArgumentParser(description="Smoke-test SayIt v2 connector framing")
    parser.add_argument("plugin_dir", type=Path)
    args = parser.parse_args()
    root = args.plugin_dir.resolve()
    manifest = validate(root)
    provider = manifest["provider"]
    realtime = next((model for model in manifest["models"] if model["category"] == "realtime"), None)
    if realtime:
        events = exchange(root, manifest, [
            {"type":"start","protocolVersion":2,"sessionId":str(uuid.uuid4()),"providerId":provider["id"],"model":realtime["id"],"sampleRate":16000,"config":provider.get("config", {}),"session":None,"permissions":manifest["runtime"].get("permissions", [])},
            {"type":"finish"},
        ])
        types = [event.get("type") for event in events]
        if "ready" not in types or "finished" not in types:
            raise RuntimeError(f"realtime smoke test missing ready/finished: {types}")
    if manifest["apiVersion"] >= 2:
        events = exchange(root, manifest, [{
            "type":"invoke","protocolVersion":2,"requestId":str(uuid.uuid4()),"operation":"action",
            "providerId":provider["id"],"config":provider.get("config", {}),"session":None,
            "permissions":manifest["runtime"].get("permissions", []),"payload":{"action":"diagnose"},
        }])
        if not events or events[-1].get("type") not in {"completed", "error"}:
            raise RuntimeError("invoke smoke test did not terminate with completed/error")
    print(f"SMOKE OK: {manifest['id']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
