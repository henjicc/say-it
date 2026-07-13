#!/usr/bin/env python3
import argparse
import json
import shutil
from pathlib import Path


def ensure_within(path: Path, workspace: Path, label: str) -> None:
    try:
        path.relative_to(workspace)
    except ValueError as error:
        raise SystemExit(f"{label} 必须位于外部插件工作目录内：{workspace}") from error


def ensure_workspace_is_external(workspace: Path, forbidden: Path) -> None:
    try:
        workspace.relative_to(forbidden)
    except ValueError:
        return
    raise SystemExit(f"插件工作目录不能位于 SayIt 仓库内：{forbidden}")


def main() -> int:
    parser = argparse.ArgumentParser(description="Create a minimal SayIt plugin package directory")
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--forbid-root", type=Path, required=True)
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()
    workspace = args.workspace.resolve()
    ensure_workspace_is_external(workspace, args.forbid_root.resolve())
    source, output = args.source.resolve(), args.output.resolve()
    ensure_within(source, workspace, "source")
    ensure_within(output, workspace, "output")
    if source == output or source in output.parents:
        raise SystemExit("output 不能是 source 或其父目录")
    manifest = json.loads((source / "manifest.json").read_text(encoding="utf-8"))
    entrypoint = Path(manifest["runtime"]["entrypoint"])
    if entrypoint.is_absolute() or ".." in entrypoint.parts or not (source / entrypoint).is_file():
        raise SystemExit("runtime.entrypoint 不存在或越界")
    if output.exists():
        if not args.force:
            raise SystemExit(f"输出目录已存在：{output}")
        shutil.rmtree(output)
    output.mkdir(parents=True)
    unsigned = dict(manifest)
    unsigned.pop("integrity", None)
    unsigned.pop("signature", None)
    (output / "manifest.json").write_text(
        json.dumps(unsigned, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    source_bin = source / entrypoint.parent
    shutil.copytree(source_bin, output / entrypoint.parent)
    print(f"PACKAGED: {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
