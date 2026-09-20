"""Verify synchronized dictionary artifacts against resources/VERSION.json."""

from __future__ import annotations

import argparse
import hashlib
import json
import sqlite3
from pathlib import Path


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def table_count(path: Path, table: str) -> int:
    with sqlite3.connect(path) as connection:
        row = connection.execute(f"SELECT COUNT(*) FROM {table}").fetchone()
    return int(row[0])


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("version_file", type=Path)
    args = parser.parse_args()
    version_file = args.version_file.resolve()
    root = version_file.parent
    manifest = json.loads(version_file.read_text(encoding="utf-8"))
    checks = {
        "system_dict.db": ["system_dict", "system_dict_trad"],
        "domains/domains.db": ["domain_words", "domain_words_trad"],
        "common.db": ["common_words"],
    }
    failed = False
    for relative, tables in checks.items():
        path = root / relative
        entry = manifest.get("dicts", {}).get(relative)
        if not path.is_file() or not entry:
            print(f"[ERROR] missing manifest or file: {relative}")
            failed = True
            continue
        actual_hash = sha256(path)
        if actual_hash != entry.get("sha256"):
            print(f"[ERROR] {relative}: sha256 manifest={entry.get('sha256')} actual={actual_hash}")
            failed = True
        for table in tables:
            actual_rows = table_count(path, table)
            expected_rows = int(entry.get("rows", {}).get(table, -1))
            if actual_rows != expected_rows:
                print(f"[ERROR] {relative}:{table}: manifest={expected_rows} actual={actual_rows}")
                failed = True
    if failed:
        return 1
    print(f"[OK] dictionary manifest verified: {manifest.get('version', 'unknown')}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
