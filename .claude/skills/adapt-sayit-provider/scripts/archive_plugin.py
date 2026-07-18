#!/usr/bin/env python3
"""把已签名的供应商插件目录打成单个 .sayit 文件。"""

import argparse
import zipfile
from pathlib import Path

from validate_plugin import validate, validate_sayit_package_declaration


def ensure_within(path: Path, work_root: Path, label: str) -> None:
    try:
        path.relative_to(work_root)
    except ValueError as error:
        raise SystemExit(f"{label} 必须位于当前工作目录内：{work_root}") from error


def main() -> int:
    parser = argparse.ArgumentParser(description="创建单文件 .sayit 供应商插件包")
    parser.add_argument("package_dir", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--work-root", type=Path, required=True)
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()

    work_root = args.work_root.resolve()
    package_dir, output = args.package_dir.resolve(), args.output.resolve()
    ensure_within(package_dir, work_root, "package_dir")
    ensure_within(output, work_root, "output")
    if output.suffix.lower() != ".sayit":
        raise SystemExit("输出文件必须使用 .sayit 后缀")
    validate(package_dir)
    validate_sayit_package_declaration(package_dir)
    if output.exists():
        if not args.force:
            raise SystemExit(f"输出文件已存在：{output}")
        output.unlink()
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix(output.suffix + ".tmp")
    try:
        with zipfile.ZipFile(temporary, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(package_dir.rglob("*")):
                if path.is_symlink():
                    raise SystemExit(f"插件包不能包含符号链接：{path}")
                if path.is_file():
                    archive.write(path, path.relative_to(package_dir).as_posix())
        temporary.replace(output)
    except Exception:
        if temporary.exists():
            temporary.unlink()
        raise
    print(f"ARCHIVED: {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
