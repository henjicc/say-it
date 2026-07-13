#!/usr/bin/env python3
"""Create a clean, external provider-plugin workspace from the bundled template."""

import argparse
import shutil
from pathlib import Path


def is_within(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False


def main() -> int:
    parser = argparse.ArgumentParser(description="Initialize an isolated SayIt plugin workspace")
    parser.add_argument("workspace", type=Path)
    parser.add_argument("--template", type=Path, required=True)
    parser.add_argument("--forbid-root", type=Path, required=True)
    args = parser.parse_args()

    workspace = args.workspace.resolve()
    template = args.template.resolve()
    forbidden = args.forbid_root.resolve()
    if is_within(workspace, forbidden):
        raise SystemExit(f"插件工作目录不能位于 SayIt 仓库内：{forbidden}")
    if not template.is_dir():
        raise SystemExit(f"插件模板不存在：{template}")
    if workspace.exists() and any(workspace.iterdir()):
        raise SystemExit(f"插件工作目录必须为空：{workspace}")

    workspace.mkdir(parents=True, exist_ok=True)
    source = workspace / "source"
    dist = workspace / "dist"
    if source.exists():
        raise SystemExit(f"工作目录已经包含 source：{source}")
    shutil.copytree(template, source)
    dist.mkdir()
    print(f"WORKSPACE READY: {workspace}")
    print(f"SOURCE: {source}")
    print(f"DIST: {dist}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
