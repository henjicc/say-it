#!/usr/bin/env python3
"""在当前工作根目录内创建干净的供应商插件工作区。"""

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
    parser = argparse.ArgumentParser(description="在当前工作目录内初始化隔离的说吧插件工作区")
    parser.add_argument("workspace", type=Path)
    parser.add_argument("--template", type=Path, required=True)
    parser.add_argument("--work-root", type=Path, required=True)
    args = parser.parse_args()

    workspace = args.workspace.resolve()
    template = args.template.resolve()
    work_root = args.work_root.resolve()
    if not is_within(workspace, work_root) or workspace == work_root:
        raise SystemExit(f"插件工作目录必须位于当前工作目录内：{work_root}")
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
