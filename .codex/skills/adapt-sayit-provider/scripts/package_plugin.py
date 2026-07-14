#!/usr/bin/env python3
"""创建只含声明、清单、JavaScript 与资源的待签名包目录。"""

import argparse
import json
import shutil
from pathlib import Path

from validate_plugin import package_files, validate_sayit_package_declaration


def ensure_within(path: Path, workspace: Path, label: str) -> None:
    try:
        path.relative_to(workspace)
    except ValueError as error:
        raise SystemExit(f"{label} 必须位于当前工作目录内：{workspace}") from error


def main() -> int:
    parser = argparse.ArgumentParser(description="创建纯 JavaScript 的「说吧！」插件包目录")
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--work-root", type=Path, required=True)
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()
    workspace = args.work_root.resolve()
    source, output = args.source.resolve(), args.output.resolve()
    ensure_within(source, workspace, "source")
    ensure_within(output, workspace, "output")
    if source == output or source in output.parents:
        raise SystemExit("output 不能是 source 或 source 的子目录")
    validate_sayit_package_declaration(source)
    manifest = json.loads((source / "manifest.json").read_text(encoding="utf-8"))
    entrypoint = Path((manifest.get("runtime") or {}).get("entrypoint", ""))
    if entrypoint.is_absolute() or ".." in entrypoint.parts or entrypoint.suffix.lower() not in {".js", ".mjs"} or not (source / entrypoint).is_file():
        raise SystemExit("runtime.entrypoint 不存在、越界或不是 JavaScript")
    package_files(source)
    if output.exists():
        if not args.force:
            raise SystemExit(f"输出目录已存在：{output}")
        shutil.rmtree(output)
    output.mkdir(parents=True)
    unsigned = dict(manifest)
    unsigned.pop("integrity", None)
    unsigned.pop("signature", None)
    (output / "manifest.json").write_text(json.dumps(unsigned, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    shutil.copy2(source / "sayit-package.json", output / "sayit-package.json")
    shutil.copytree(source / "connector", output / "connector")
    package_files(output)
    print(f"PACKAGED: {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
