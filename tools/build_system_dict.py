#!/usr/bin/env python3
"""构建系统词库 SQLite 数据库。

输入: 原始词表文件（每行: pinyin\tword\tfrequency）
输出: resources/system_dict.db

用法:
    python tools/build_system_dict.py resources/raw_dict.txt resources/system_dict.db
"""

import sqlite3
import sys
import os


def create_db(db_path: str):
    conn = sqlite3.connect(db_path)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS system_dict (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            pinyin TEXT NOT NULL,
            word TEXT NOT NULL,
            frequency INTEGER DEFAULT 0
        )
    """)
    conn.execute("CREATE INDEX IF NOT EXISTS idx_pinyin ON system_dict(pinyin)")
    return conn


def import_words(conn: sqlite3.Connection, raw_path: str):
    count = 0
    with open(raw_path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            parts = line.split("\t")
            if len(parts) == 3:
                pinyin, word, freq = parts
                freq = int(freq)
            elif len(parts) == 2:
                pinyin, word = parts
                freq = 0
            else:
                print(f"跳过无效行: {line}")
                continue
            conn.execute(
                "INSERT INTO system_dict (pinyin, word, frequency) VALUES (?, ?, ?)",
                (pinyin.strip().lower(), word.strip(), freq),
            )
            count += 1

    conn.commit()
    print(f"导入 {count} 条词条")


def main():
    if len(sys.argv) != 3:
        print(f"用法: {sys.argv[0]} <raw_dict.txt> <output.db>")
        sys.exit(1)

    raw_path = sys.argv[1]
    db_path = sys.argv[2]

    if not os.path.exists(raw_path):
        print(f"错误: 词表文件不存在: {raw_path}")
        sys.exit(1)

    os.makedirs(os.path.dirname(db_path) or ".", exist_ok=True)
    conn = create_db(db_path)
    import_words(conn, raw_path)
    conn.close()

    db_size = os.path.getsize(db_path)
    print(f"系统词库已生成: {db_path} ({db_size:,} bytes)")


if __name__ == "__main__":
    main()
