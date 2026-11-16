"""Verify synchronized dictionary artifacts against resources/VERSION.json.

覆盖 system_dict / domains / common / bigram 四份产物：
- 文件存在性
- sha256 与 VERSION.json 登记一致（防传错、截断、旧版本覆盖）
- 关键表行数与登记一致

`bigram.db` 是可选资源（缺失时引擎按无表降级），因此它只在 VERSION.json
登记后才被校验；未登记视为旧版本兼容，跳过而不报错。
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sqlite3
from pathlib import Path

# 可选资源：未在 VERSION.json 登记时跳过校验（旧版本兼容）
OPTIONAL = {"bigram.db"}


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
        # 搭配表：引擎在 ActivateEx 时按 DLL 同目录读取，传错或截断会导致
        # 上下文排序静默退化（不报错、只是不生效），必须在此拦住。
        "bigram.db": ["context_prior"],
    }
    failed = False
    for relative, tables in checks.items():
        path = root / relative
        entry = manifest.get("dicts", {}).get(relative)
        if entry is None:
            if relative in OPTIONAL:
                print(f"[SKIP] {relative}: not in manifest (optional resource)")
                continue
            print(f"[ERROR] missing manifest entry: {relative}")
            failed = True
            continue
        if not path.is_file():
            print(f"[ERROR] missing file: {relative}")
            failed = True
            continue
        actual_hash = sha256(path)
        if actual_hash != entry.get("sha256"):
            print(f"[ERROR] {relative}: sha256 manifest={entry.get('sha256')} actual={actual_hash}")
            failed = True
        for table in tables:
            try:
                actual_rows = table_count(path, table)
            except sqlite3.Error as error:
                print(f"[ERROR] {relative}:{table}: {error}")
                failed = True
                continue
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
