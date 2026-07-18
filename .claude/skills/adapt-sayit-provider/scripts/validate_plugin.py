#!/usr/bin/env python3
"""校验「说吧！」JavaScript 供应商插件目录。"""

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
COOKIE_NAME = re.compile(r"^[!#$%&'*+\-.^_`|~0-9A-Za-z]{1,128}$")
HOST = re.compile(r"^(?:\*\.)?[A-Za-z0-9](?:[A-Za-z0-9.-]*[A-Za-z0-9])?$")
PERMISSIONS = {"network", "browserSession", "cookies"}
NATIVE_EXTENSIONS = {".exe", ".dll", ".so", ".dylib", ".com", ".scr", ".msi", ".node", ".wasm"}
NATIVE_MAGICS = (
    b"MZ", b"\x7fELF", b"\xfe\xed\xfa\xce", b"\xce\xfa\xed\xfe",
    b"\xfe\xed\xfa\xcf", b"\xcf\xfa\xed\xfe", b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca",
)
PACKAGE_DECLARATION = "sayit-package.json"


def fail(message: str) -> None:
    raise ValueError(message)


def validate_sayit_package_declaration(root: Path) -> None:
    path = root / PACKAGE_DECLARATION
    if not path.is_file():
        fail(f"说吧包缺少 {PACKAGE_DECLARATION}")
    declaration = json.loads(path.read_text(encoding="utf-8"))
    if declaration != {"formatVersion": 1, "kind": "provider-plugin", "entry": "manifest.json"}:
        fail("供应商插件必须声明 formatVersion=1、kind=provider-plugin、entry=manifest.json")


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
    runtime.setdefault("kind", "javascript")
    runtime.setdefault("hostApiVersion", 1)
    runtime.setdefault("permissions", [])
    runtime.setdefault("network", {})
    runtime["network"].setdefault("allowedHosts", [])
    return value


def signing_payload(data: dict) -> bytes:
    value = normalized_manifest(data)
    if "signature" in value:
        value["signature"]["value"] = ""
    canonical = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return b"sayit-plugin-signature-v1\n" + canonical.encode("utf-8")


def package_files(root: Path) -> set[str]:
    files: set[str] = set()
    for path in root.rglob("*"):
        if path.is_symlink():
            fail(f"插件包不能包含符号链接：{path}")
        if not path.is_file():
            continue
        relative = path.relative_to(root).as_posix()
        if path.suffix.lower() in NATIVE_EXTENSIONS:
            fail(f"插件包不能包含原生可执行文件、动态库或 WASM：{relative}")
        prefix = path.read_bytes()[:4]
        if any(prefix.startswith(magic) for magic in NATIVE_MAGICS):
            fail(f"插件包检测到原生二进制：{relative}")
        if relative != "manifest.json":
            files.add(relative)
    return files


def validate_integrity(root: Path, data: dict) -> str:
    integrity, signature = data.get("integrity"), data.get("signature")
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
    root = root.resolve()
    validate_sayit_package_declaration(root)
    manifest_path = root / "manifest.json"
    if not manifest_path.is_file():
        fail("manifest.json 不存在")
    data = json.loads(manifest_path.read_text(encoding="utf-8"))
    if data.get("apiVersion") != 3:
        fail("apiVersion 必须为 3；旧进程插件不兼容")
    provider = data.get("provider") or {}
    for label, value in (("插件", data.get("id")), ("供应商", provider.get("id"))):
        if not isinstance(value, str) or not ID.fullmatch(value):
            fail(f"{label} ID 不合法：{value!r}")
    if not str(data.get("name", "")).strip() or not str(data.get("version", "")).strip():
        fail("name 和 version 不能为空")
    capabilities = set(provider.get("capabilities", []))
    if not capabilities & {"asr", "translation", "customization"}:
        fail("provider.capabilities 未声明受支持能力")
    if not isinstance(provider.get("config", {}), dict):
        fail("provider.config 必须是 JSON 对象")
    field_keys: set[str] = set()
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
    if runtime.get("kind", "javascript") != "javascript" or runtime.get("hostApiVersion", 1) != 1:
        fail("runtime 必须为 javascript/hostApiVersion 1")
    permissions = set(runtime.get("permissions", []))
    if permissions - PERMISSIONS:
        fail(f"未知权限：{sorted(permissions - PERMISSIONS)}")
    allowed_hosts = (runtime.get("network") or {}).get("allowedHosts", [])
    if "network" in permissions and not allowed_hosts:
        fail("声明 network 权限时 allowedHosts 不能为空")
    if not isinstance(allowed_hosts, list) or any(not isinstance(host, str) or not HOST.fullmatch(host) or "/" in host or ":" in host for host in allowed_hosts):
        fail("allowedHosts 只能包含精确主机或 *.example.com 子域规则")
    entrypoint = runtime.get("entrypoint")
    if not isinstance(entrypoint, str) or not entrypoint:
        fail("runtime.entrypoint 不能为空")
    entry_path = Path(entrypoint)
    if entry_path.is_absolute() or ".." in entry_path.parts or entry_path.suffix.lower() not in {".js", ".mjs"}:
        fail("entrypoint 必须是插件目录内的 .js 或 .mjs 相对路径")
    resolved_entry = (root / entry_path).resolve()
    try:
        resolved_entry.relative_to(root)
    except ValueError:
        fail("entrypoint 不能通过符号链接跳出插件目录")
    if not resolved_entry.is_file():
        fail(f"插件入口不存在：{entrypoint}")

    browser = data.get("browserSession")
    if browser:
        if not {"browserSession", "cookies"}.issubset(permissions):
            fail("browserSession 必须声明 browserSession 和 cookies 权限")
        urls = [browser.get("loginUrl"), *browser.get("allowedUrls", [])]
        if len(urls) < 2 or any(not isinstance(url, str) or not url.startswith("https://") for url in urls):
            fail("网页登录 URL 必须是 HTTPS，且 allowedUrls 不能为空")
        if len(browser.get("initializationScript", "")) > 64 * 1024:
            fail("initializationScript 超过 64 KiB")
        required_cookie_names = browser.get("requiredCookieNames", [])
        if (
            not isinstance(required_cookie_names, list)
            or len(required_cookie_names) != len(set(required_cookie_names))
            or any(not isinstance(name, str) or not COOKIE_NAME.fullmatch(name) for name in required_cookie_names)
        ):
            fail("requiredCookieNames 只能包含不重复的合法 Cookie 名")
        required = {"openLogin", "syncSession", "clearSession"}
        if not required.issubset(actions):
            fail(f"browserSession 必须声明操作：{sorted(required)}")

    models = data.get("models")
    if not isinstance(models, list) or not models:
        fail("至少声明一个模型")
    seen: set[str] = set()
    for model in models:
        model_id = model.get("id")
        if not isinstance(model_id, str) or not ID.fullmatch(model_id) or model_id in seen:
            fail(f"模型 ID 不合法或重复：{model_id!r}")
        seen.add(model_id)
        if model.get("providerId") != provider.get("id"):
            fail(f"模型 {model_id} 的 providerId 不匹配")
        category, protocol, scenes = model.get("category"), model.get("protocol"), set(model.get("scenes", []))
        valid = (
            category == "realtime" and "asr" in capabilities and protocol == "plugin-realtime-v1" and bool(scenes & {"dictationRealtime", "subtitles"})
        ) or (
            category == "file" and "asr" in capabilities and protocol == "plugin-file-v1" and bool(scenes & {"dictationFile", "transcription"})
        ) or (
            category == "translation" and "translation" in capabilities and protocol == "plugin-translation-v1" and "subtitleTranslation" in scenes
        )
        if not valid:
            fail(f"模型 {model_id} 的类别、协议或场景组合不受支持")
    package_files(root)
    data["_trust"] = validate_integrity(root, data)
    return data


def main() -> int:
    parser = argparse.ArgumentParser(description="校验「说吧！」JavaScript 供应商插件")
    parser.add_argument("plugin_dir", type=Path)
    args = parser.parse_args()
    try:
        data = validate(args.plugin_dir)
    except Exception as error:
        print(f"INVALID: {error}", file=sys.stderr)
        return 1
    print(f"VALID: {data['id']} {data['version']} ({len(data['models'])} models, {data['_trust']})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
