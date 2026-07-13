#!/usr/bin/env python3
import argparse
import os
import shutil
import sys
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
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()
    source = args.plugin_dir.resolve()
    try:
        manifest = validate(source)
        destination = (args.destination or default_plugins_dir()).resolve()
        destination.mkdir(parents=True, exist_ok=True)
        target = destination / manifest["id"]
        staging = destination / f".{manifest['id']}.installing"
        if target.exists() and not args.force:
            raise RuntimeError(f"插件已存在：{target}；更新时显式使用 --force")
        if staging.exists():
            shutil.rmtree(staging)
        shutil.copytree(source, staging, ignore=shutil.ignore_patterns("target", ".git", "__pycache__", "*.pdb"))
        validate(staging)
        if target.exists():
            shutil.rmtree(target)
        staging.replace(target)
    except Exception as error:
        print(f"INSTALL FAILED: {error}", file=sys.stderr)
        return 1
    print(f"INSTALLED: {target}")
    print("Restart SayIt or reload provider plugins before testing.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
