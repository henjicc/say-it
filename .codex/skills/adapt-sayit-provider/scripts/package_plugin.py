#!/usr/bin/env python3
import argparse
import json
import shutil
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description="Create a minimal SayIt plugin package directory")
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()
    source, output = args.source.resolve(), args.output.resolve()
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
