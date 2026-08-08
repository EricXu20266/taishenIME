# -*- coding: utf-8 -*-
"""雾凇 symbols_v.yaml → Rust SYMBOL_TABLE 常量生成器
排除：vhelp/vlist 索引、单字母拉丁变体（va-vz/vA-vZ）、数字变体（v0-v10）
"""
import io
import re

SRC = r"E:\AllinDeepSeek\rime-ice\symbols_v.yaml"
OUT = r"E:\AllinDeepSeek\taishenIME\tools\symbol_table_rust.txt"

src = io.open(SRC, encoding="utf-8").read()
entries = []
for ln in src.split("\n"):
    m = re.match(r"^  'v([A-Za-z0-9]+)':\s*\[\s*(.*?)\s*\]\s*$", ln)
    if not m:
        continue
    code, body = m.group(1), m.group(2)
    if code in ("help", "list"):
        continue
    if re.match(r"^[a-zA-Z]$", code) or re.match(r"^[0-9]+$", code):
        continue
    if re.search(r"[A-Z]", code):
        continue  # 大写分类码（拉丁合字变体）：引擎小写化后无法访问，排除
    items = []
    for part in body.split(","):
        p = part.strip()
        if not p:
            continue
        if len(p) >= 2 and p[0] == "'" and p[-1] == "'":
            p = p[1:-1]
        elif len(p) >= 2 and p[0] == '"' and p[-1] == '"':
            p = p[1:-1]
        if p:
            items.append(p)
    entries.append((code, items))

print(f"分类数: {len(entries)}")
total = sum(len(v) for _, v in entries)
print(f"符号总数: {total}")

out = io.open(OUT, "w", encoding="utf-8")
out.write("const SYMBOL_TABLE: &[(&str, &[&str])] = &[\n")
for code, items in entries:
    rust_items = ", ".join('"%s"' % s for s in items)
    out.write('    ("%s", &[%s]),\n' % (code, rust_items))
out.write("];\n")
out.close()
print("已写出:", OUT)
