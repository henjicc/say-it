#!/usr/bin/env python3
import argparse
import base64
import hashlib
import json
import re
import sys
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

ID = re.compile(r"^[a-z0-9.-]{1,64}$")
ACTION_ID = re.compile(r"^[A-Za-z0-9.-]{1,64}$")
PERMISSIONS = {"network", "browserSession", "cookies"}
BUILTIN_PROVIDERS = {"funasr"}
BUILTIN_MODELS = {
    "fun-asr-realtime-2026-02-28", "fun-asr-realtime", "qwen3-asr-flash-realtime-2026-02-10",
    "qwen3-asr-flash-realtime", "fun-asr-flash-2026-06-15", "qwen3-asr-flash",
    "qwen3-asr-flash-2026-02-10", "fun-asr", "qwen3-asr-flash-filetrans",
}
PACKAGE_DECLARATION = "sayit-package.json"


def fail(message: str) -> None:
    raise ValueError(message)


def validate_sayit_package_declaration(root: Path) -> None:
    path = root / PACKAGE_DECLARATION
    if not path.is_file():
        fail(f"说吧包缺少 {PACKAGE_DECLARATION}")
    try:
        declaration = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"说吧包声明格式错误：{error}")
    if declaration != {
        "formatVersion": 1,
        "kind": "provider-plugin",
        "entry": "manifest.json",
    }:
        fail("当前供应商插件必须声明 formatVersion=1、kind=provider-plugin、entry=manifest.json")


def normalized_manifest(data: dict) -> dict:
    value = json.loads(json.dumps(data))
    provider = value.setdefault("provider", {})
    provider.setdefault("authKind", "custom")
    provider.setdefault("capabilities", ["asr"])
    provider.setdefault("config", {})
    provider.setdefault("configFields", [])
    provider.setdefault("actions", [])
    value.setdefault("models", [])
    runtime = value.setdefault("runtime", {})
    runtime.setdefault("kind", "process")
    runtime.setdefault("args", [])
    runtime.setdefault("protocolVersion", 2)
    runtime.setdefault("permissions", [])
    return value


def signing_payload(data: dict) -> bytes:
    value = normalized_manifest(data)
    if "signature" in value:
        value["signature"]["value"] = ""
    canonical = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return b"sayit-plugin-signature-v1\n" + canonical.encode("utf-8")


def package_files(root: Path) -> set[str]:
    files = set()
    for path in root.rglob("*"):
        if path.is_symlink():
            fail(f"插件包不能包含符号链接：{path}")
        if path.is_file() and path.name != "manifest.json":
            files.add(path.relative_to(root).as_posix())
    return files


def validate_integrity(root: Path, data: dict) -> str:
    integrity = data.get("integrity")
    signature = data.get("signature")
    if not integrity:
        if signature:
            fail("签名插件必须提供 integrity")
        return "unsigned"
    if integrity.get("algorithm", "").lower() != "sha256":
        fail("integrity.algorithm 必须为 sha256")
    declared = integrity.get("files")
    if not isinstance(declared, dict) or not declared:
        fail("integrity.files 不能为空")
    actual = package_files(root)
    if actual != set(declared):
        fail(f"完整性清单与文件不一致：未声明={sorted(actual - set(declared))}，不存在={sorted(set(declared) - actual)}")
    for relative, expected in declared.items():
        digest = hashlib.sha256((root / relative).read_bytes()).hexdigest()
        if digest.lower() != str(expected).strip().lower():
            fail(f"文件哈希不匹配：{relative}")
    if not signature:
        return "integrity-only"
    if signature.get("algorithm", "").lower() != "ed25519":
        fail("signature.algorithm 必须为 ed25519")
    public = base64.b64decode(signature.get("publicKey", ""), validate=True)
    signed = base64.b64decode(signature.get("value", ""), validate=True)
    Ed25519PublicKey.from_public_bytes(public).verify(signed, signing_payload(data))
    return "signed"


def validate(root: Path) -> dict:
    manifest_path = root / "manifest.json"
    if not manifest_path.is_file():
        fail("manifest.json 不存在")
    data = json.loads(manifest_path.read_text(encoding="utf-8"))
    if data.get("apiVersion") not in {1, 2}:
        fail("apiVersion 必须为 1 或 2")
    for label, value in (("插件", data.get("id")), ("供应商", data.get("provider", {}).get("id"))):
        if not isinstance(value, str) or not ID.fullmatch(value):
            fail(f"{label} ID 不合法：{value!r}")
    if not str(data.get("name", "")).strip() or not str(data.get("version", "")).strip():
        fail("name 和 version 不能为空")
    provider = data.get("provider") or {}
    if provider.get("id") in BUILTIN_PROVIDERS:
        fail("插件供应商 ID 不能覆盖内置供应商")
    capabilities = set(provider.get("capabilities", []))
    if not capabilities & {"asr", "translation", "customization"}:
        fail("provider.capabilities 未声明受支持能力")
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
    actions = provider.get("actions", [])
    if len(actions) != len(set(actions)) or any(not isinstance(action, str) or not ACTION_ID.fullmatch(action) for action in actions):
        fail("provider.actions 包含非法或重复操作 ID")
    runtime = data.get("runtime") or {}
    protocol = runtime.get("protocolVersion", 2)
    if runtime.get("kind", "process") != "process" or protocol not in {1, 2}:
        fail("runtime 必须为 process/protocolVersion 1 或 2")
    permissions = set(runtime.get("permissions", []))
    if permissions - PERMISSIONS:
        fail(f"未知权限：{sorted(permissions - PERMISSIONS)}")
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
    browser = data.get("browserSession")
    if browser:
        if not {"browserSession", "cookies"}.issubset(permissions):
            fail("browserSession 配置必须声明 browserSession 和 cookies 权限")
        urls = [browser.get("loginUrl"), *browser.get("allowedUrls", [])]
        if len(urls) < 2 or any(not isinstance(url, str) or not url.startswith("https://") for url in urls):
            fail("网页登录 URL 必须是 HTTPS，且 allowedUrls 不能为空")
        if len(browser.get("initializationScript", "")) > 64 * 1024:
            fail("initializationScript 超过 64 KiB")
        required = {"openLogin", "syncSession", "clearSession"}
        if not required.issubset(actions):
            fail(f"browserSession 必须声明操作：{sorted(required)}")
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
        category, model_protocol, scenes = model.get("category"), model.get("protocol"), set(model.get("scenes", []))
        valid = (
            category == "realtime" and "asr" in capabilities and model_protocol in {"process-jsonl-v1", "process-jsonl-v2"}
            and bool(scenes & {"dictationRealtime", "subtitles"})
        ) or (
            data["apiVersion"] >= 2 and category == "file" and "asr" in capabilities
            and model_protocol == "process-file-v2" and bool(scenes & {"dictationFile", "transcription"})
        ) or (
            data["apiVersion"] >= 2 and category == "translation" and "translation" in capabilities
            and model_protocol == "process-translation-v2" and "subtitleTranslation" in scenes
        )
        if not valid:
            fail(f"模型 {model_id} 的类别、协议或场景组合不受支持")
    data["_trust"] = validate_integrity(root, data)
    return data


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate a SayIt provider plugin")
    parser.add_argument("plugin_dir", type=Path)
    args = parser.parse_args()
    try:
        data = validate(args.plugin_dir.resolve())
    except Exception as error:
        print(f"INVALID: {error}", file=sys.stderr)
        return 1
    print(f"VALID: {data['id']} {data['version']} ({len(data['models'])} models, {data['_trust']})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
