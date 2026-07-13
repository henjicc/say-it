#!/usr/bin/env python3
import argparse
import json
import os
import shutil
import sys
import uuid
from pathlib import Path

from validate_plugin import validate


def default_plugins_dir() -> Path:
    local = os.environ.get("LOCALAPPDATA")
    if not local:
        raise RuntimeError("LOCALAPPDATA 未设置，请使用 --destination")
    return Path(local) / "com.henjicc.sayit" / "plugins"


def main() -> int:
    parser = argparse.ArgumentParser(description="Install a validated SayIt provider plugin")
    parser.add_argument("plugin_dir", type=Path)
    parser.add_argument("--destination", type=Path)
    parser.add_argument("--allow-unsigned", action="store_true")
    parser.add_argument("--trust-key", action="store_true")
    args = parser.parse_args()
    source = args.plugin_dir.resolve()
    stage = None
    previous = None
    target = None
    try:
        manifest = validate(source)
        trust = manifest.pop("_trust")
        if trust in {"unsigned", "integrity-only"} and not args.allow_unsigned:
            raise RuntimeError("插件未签名；如确认来源可信，显式使用 --allow-unsigned")
        destination = (args.destination or default_plugins_dir()).resolve()
        destination.mkdir(parents=True, exist_ok=True)
        trust_file = destination.parent / "trusted-plugin-keys.json"
        trust_data = json.loads(trust_file.read_text(encoding="utf-8")) if trust_file.exists() else {"keys": {}}
        signature = manifest.get("signature")
        if signature:
            trusted = trust_data.setdefault("keys", {}).get(signature["keyId"]) == signature["publicKey"]
            if not trusted and not args.trust_key:
                raise RuntimeError("插件签名有效，但密钥未受信任；确认发布者后使用 --trust-key")
            if not trusted:
                trust_data["keys"][signature["keyId"]] = signature["publicKey"]
                temp_trust = trust_file.with_suffix(".tmp")
                temp_trust.write_text(json.dumps(trust_data, ensure_ascii=False, indent=2), encoding="utf-8")
                temp_trust.replace(trust_file)
        target = destination / manifest["id"]
        stage = destination / f".install-{manifest['id']}-{uuid.uuid4()}"
        shutil.copytree(source, stage)
        validate(stage)
        if target.exists():
            current = json.loads((target / "manifest.json").read_text(encoding="utf-8"))
            backup_root = destination.parent / "plugin-backups"
            backup_root.mkdir(parents=True, exist_ok=True)
            previous = backup_root / f"{manifest['id']}--{current.get('version', 'unknown')}--{uuid.uuid4()}"
            target.replace(previous)
        stage.replace(target)
    except Exception as error:
        if stage and stage.exists():
            shutil.rmtree(stage)
        if previous and previous.exists() and target and not target.exists():
            previous.replace(target)
        print(f"INSTALL FAILED: {error}", file=sys.stderr)
        return 1
    print(f"INSTALLED: {target}")
    print("Restart SayIt or use 重新扫描 before testing.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
