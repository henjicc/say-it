#!/usr/bin/env python3
import argparse
import json
import re
import sys
from pathlib import Path

ID = re.compile(r"^[a-z0-9.-]{1,64}$")
PERMISSIONS = {"network", "browserSession", "cookies"}
SCENES = {"dictationRealtime", "subtitles"}
BUILTIN_PROVIDERS = {"funasr"}
BUILTIN_MODELS = {
    "fun-asr-realtime-2026-02-28", "fun-asr-realtime", "qwen3-asr-flash-realtime-2026-02-10",
    "qwen3-asr-flash-realtime", "fun-asr-flash-2026-06-15", "qwen3-asr-flash",
    "qwen3-asr-flash-2026-02-10", "fun-asr", "qwen3-asr-flash-filetrans",
}


def fail(message: str) -> None:
    raise ValueError(message)


def validate(root: Path) -> dict:
    manifest_path = root / "manifest.json"
    if not manifest_path.is_file():
        fail("manifest.json 不存在")
    data = json.loads(manifest_path.read_text(encoding="utf-8"))
    if data.get("apiVersion") != 1:
        fail("apiVersion 必须为 1")
    for label, value in (("插件", data.get("id")), ("供应商", data.get("provider", {}).get("id"))):
        if not isinstance(value, str) or not ID.fullmatch(value):
            fail(f"{label} ID 不合法：{value!r}")
    if not str(data.get("name", "")).strip() or not str(data.get("version", "")).strip():
        fail("name 和 version 不能为空")
    provider = data.get("provider") or {}
    if provider.get("id") in BUILTIN_PROVIDERS:
        fail("插件供应商 ID 不能覆盖内置供应商")
    if provider.get("capabilities") != ["asr"] and "asr" not in provider.get("capabilities", []):
        fail("provider.capabilities 必须包含 asr")
    if not isinstance(provider.get("config", {}), dict):
        fail("provider.config 必须是 JSON 对象")
    field_keys = set()
    for field in provider.get("configFields", []):
        key = field.get("key")
        if not isinstance(key, str) or not key or key in field_keys:
            fail(f"配置字段 key 为空或重复：{key!r}")
        field_keys.add(key)
        if field.get("fieldType") not in {"text", "password", "number", "boolean"}:
            fail(f"配置字段 {key} 的 fieldType 不受支持")
    runtime = data.get("runtime") or {}
    if runtime.get("kind") != "process" or runtime.get("protocolVersion", 1) != 1:
        fail("runtime 必须为 process/protocolVersion 1")
    permissions = runtime.get("permissions", [])
    unknown = set(permissions) - PERMISSIONS
    if unknown:
        fail(f"未知权限：{sorted(unknown)}")
    entrypoint = runtime.get("entrypoint")
    if not isinstance(entrypoint, str) or not entrypoint:
        fail("runtime.entrypoint 不能为空")
    entry_path = Path(entrypoint)
    if entry_path.is_absolute() or ".." in entry_path.parts:
        fail("entrypoint 必须位于插件目录内")
    resolved_entry = (root / entry_path).resolve()
    try:
        resolved_entry.relative_to(root.resolve())
    except ValueError:
        fail("entrypoint 不能通过符号链接跳出插件目录")
    if not resolved_entry.is_file():
        fail(f"插件入口不存在：{entrypoint}")
    models = data.get("models")
    if not isinstance(models, list) or not models:
        fail("至少声明一个模型")
    seen = set()
    for model in models:
        model_id = model.get("id")
        if not isinstance(model_id, str) or not ID.fullmatch(model_id) or model_id in seen:
            fail(f"模型 ID 不合法或重复：{model_id!r}")
        if model_id in BUILTIN_MODELS:
            fail(f"模型 ID 不能覆盖内置模型：{model_id}")
        seen.add(model_id)
        if model.get("providerId") != provider.get("id"):
            fail(f"模型 {model_id} 的 providerId 不匹配")
        if model.get("category") != "realtime" or model.get("protocol") != "process-jsonl-v1":
            fail(f"模型 {model_id} 必须使用 realtime/process-jsonl-v1")
        if not (set(model.get("scenes", [])) & SCENES):
            fail(f"模型 {model_id} 没有可用实时场景")
    return data


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate a SayIt provider plugin")
    parser.add_argument("plugin_dir", type=Path)
    args = parser.parse_args()
    try:
        data = validate(args.plugin_dir.resolve())
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"INVALID: {error}", file=sys.stderr)
        return 1
    print(f"VALID: {data['id']} {data['version']} ({len(data['models'])} models)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
