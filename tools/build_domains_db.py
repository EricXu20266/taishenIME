#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
domains/*.txt → domains.db 构建工具
═══════════════════════════════════════
扫描 resources/domains/*.txt → 写入 domains.db（SQLite）。
引擎启动时优先读 domains.db，不存在则回退 txt 扫描。

用法：
    python tools/build_domains_db.py [domains_dir] [output_db]
    # 默认 domains_dir=resources/domains  output_db=resources/domains/domains.db
"""

import os, sqlite3, sys

def build(domains_dir: str, db_path: str):
    if not os.path.isdir(domains_dir):
        print(f"错误：{domains_dir} 不是目录")
        sys.exit(1)

    conn = sqlite3.connect(db_path)
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA synchronous=NORMAL")
    conn.executescript("""
        DROP TABLE IF EXISTS domain_words;
        CREATE TABLE domain_words (
            word        TEXT NOT NULL,
            pinyin      TEXT NOT NULL,
            domain_id   INTEGER NOT NULL,
            domain_name TEXT NOT NULL,
            PRIMARY KEY (word, domain_id)
        );
        CREATE INDEX IF NOT EXISTS idx_dw_pinyin ON domain_words(pinyin);
        CREATE INDEX IF NOT EXISTS idx_dw_domain ON domain_words(domain_id);
    """)

    txt_files = sorted(
        f for f in os.listdir(domains_dir)
        if f.endswith('.txt') and f != 'conversation.txt'  # conversation 独立加载，不进 DB（先保持与现有架构一致，后续可合入）
    )
    # 实际所有 txt 都进 DB——上面过滤是错的，修正：
    txt_files = sorted(f for f in os.listdir(domains_dir) if f.endswith('.txt'))

    total = 0
    for domain_id, fn in enumerate(txt_files):
        path = os.path.join(domains_dir, fn)
        domain_name = fn.rsplit('.', 1)[0]  # computer.txt → computer
        count = 0
        with open(path, encoding='utf-8') as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith('#'):
                    continue
                parts = line.split()
                if len(parts) < 2:
                    continue
                word, pinyin = parts[0], parts[1]
                if not pinyin or not pinyin.isascii() or not pinyin.islower():
                    continue
                conn.execute(
                    "INSERT OR IGNORE INTO domain_words VALUES (?,?,?,?)",
                    (word, pinyin, domain_id, domain_name)
                )
                count += 1
        conn.commit()
        total += count
        print(f"  {fn}: {count} 词 (domain_id={domain_id})")

    conn.commit()
    conn.close()

    db_size = os.path.getsize(db_path)
    print(f"\ndomains.db 构建完成: {len(txt_files)} 领域, {total} 词, {db_size/1024/1024:.1f} MB")
    print(f"路径: {db_path}")

if __name__ == '__main__':
    domains_dir = sys.argv[1] if len(sys.argv) > 1 else 'resources/domains'
    db_path = sys.argv[2] if len(sys.argv) > 2 else os.path.join(domains_dir, 'domains.db')
    build(os.path.abspath(domains_dir), os.path.abspath(db_path))
