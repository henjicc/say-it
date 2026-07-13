#!/usr/bin/env python3
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from validate_plugin import signing_payload, validate


def load_or_create_key(path: Path) -> Ed25519PrivateKey:
    if path.exists():
        return serialization.load_pem_private_key(path.read_bytes(), password=None)
    key = Ed25519PrivateKey.generate()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(key.private_bytes(
        serialization.Encoding.PEM,
        serialization.PrivateFormat.PKCS8,
        serialization.NoEncryption(),
    ))
    try:
        os.chmod(path, 0o600)
    except OSError:
        pass
    print(f"CREATED PRIVATE KEY: {path}")
    return key


def ensure_within(path: Path, workspace: Path, label: str) -> None:
    try:
        path.relative_to(workspace)
    except ValueError as error:
        raise SystemExit(f"{label} 必须位于外部插件工作目录内：{workspace}") from error


def main() -> int:
    parser = argparse.ArgumentParser(description="Hash and Ed25519-sign a SayIt plugin package")
    parser.add_argument("plugin_dir", type=Path)
    parser.add_argument("--private-key", type=Path, required=True)
    parser.add_argument("--key-id", required=True)
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--forbid-root", type=Path, required=True)
    args = parser.parse_args()
    workspace = args.workspace.resolve()
    forbidden = args.forbid_root.resolve()
    try:
        workspace.relative_to(forbidden)
        raise SystemExit(f"插件工作目录不能位于 SayIt 仓库内：{forbidden}")
    except ValueError:
        pass
    root = args.plugin_dir.resolve()
    private_key_path = args.private_key.resolve()
    ensure_within(root, workspace, "plugin_dir")
    ensure_within(private_key_path, workspace, "private-key")
    try:
        private_key_path.relative_to(root)
        raise SystemExit("私钥不能放在插件包目录内")
    except ValueError:
        pass
    manifest_path = root / "manifest.json"
    data = json.loads(manifest_path.read_text(encoding="utf-8"))
    data.pop("integrity", None)
    data.pop("signature", None)
    files = {}
    for path in sorted(path for path in root.rglob("*") if path.is_file() and path.name != "manifest.json"):
        files[path.relative_to(root).as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
    data["integrity"] = {"algorithm": "sha256", "files": files}
    key = load_or_create_key(private_key_path)
    public = key.public_key().public_bytes(
        serialization.Encoding.Raw, serialization.PublicFormat.Raw
    )
    data["signature"] = {
        "algorithm": "ed25519",
        "keyId": args.key_id,
        "publicKey": base64.b64encode(public).decode("ascii"),
        "value": "",
    }
    data["signature"]["value"] = base64.b64encode(key.sign(signing_payload(data))).decode("ascii")
    manifest_path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    validated = validate(root)
    print(f"SIGNED: {validated['id']} {validated['version']} ({validated['_trust']})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
