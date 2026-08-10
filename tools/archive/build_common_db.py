#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
common_dict.txt → common.db 构建工具
═══════════════════════════════════
解析 resources/common_dict.txt → 写入 resources/common.db（SQLite）。
引擎启动时优先读 common.db，不存在则回退 txt 扫描。

用法：
    python tools/build_common_db.py [common_txt] [output_db]
    # 默认 common_txt=resources/common_dict.txt  output_db=resources/common.db
"""

import os, sqlite3, sys

def build(txt_path: str, db_path: str):
    if not os.path.isfile(txt_path):
        print(f"错误：{txt_path} 不存在")
        sys.exit(1)

    conn = sqlite3.connect(db_path)
    conn.executescript("""
        DROP TABLE IF EXISTS common_words;
        CREATE TABLE common_words (
            rank    INTEGER NOT NULL PRIMARY KEY,
            pinyin  TEXT NOT NULL,
            word    TEXT NOT NULL
        );
    """)

    count = 0
    with open(txt_path, encoding='utf-8') as f:
        for line in f:
            line = line.lstrip('\ufeff').strip()
            if not line or line.startswith('#'):
                continue
            parts = line.split('\t')
            if len(parts) < 2:
                continue
            pinyin = parts[0].strip().lower()
            word = parts[1].strip()
            if not pinyin or not word:
                continue
            conn.execute(
                "INSERT INTO common_words (rank, pinyin, word) VALUES (?,?,?)",
                (count, pinyin, word)
            )
            count += 1

    conn.commit()
    conn.close()

    db_size = os.path.getsize(db_path)
    print(f"common.db 构建完成: {count} 条, {db_size/1024:.1f} KB")
    print(f"路径: {db_path}")

if __name__ == '__main__':
    txt_path = sys.argv[1] if len(sys.argv) > 1 else 'resources/common_dict.txt'
    db_path = sys.argv[2] if len(sys.argv) > 2 else 'resources/common.db'
    build(os.path.abspath(txt_path), os.path.abspath(db_path))
