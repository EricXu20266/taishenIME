# -*- coding: utf-8 -*-
"""把生成的 SYMBOL_TABLE 替换进 engine/src/symbol.rs，并合并 yf 分类（中文月份+日文月份）"""
import io
import re

SYMBOL_RS = r"E:\AllinDeepSeek\taishenIME\engine\src\symbol.rs"
GEN = r"E:\AllinDeepSeek\taishenIME\tools\symbol_table_rust.txt"

rs = io.open(SYMBOL_RS, encoding="utf-8").read()
table = io.open(GEN, encoding="utf-8").read().strip()

# yf 合并：中文月份（我们原有）+ 日文月份（雾凇）
CN_MONTHS = ["一月", "二月", "三月", "四月", "五月", "六月",
             "七月", "八月", "九月", "十月", "十一月", "十二月"]
JP_MONTHS = ["㋀", "㋁", "㋂", "㋃", "㋄", "㋅", "㋆", "㋇", "㋈", "㋉", "㋊", "㋋"]
merged_yf = '    ("yf", &["' + '", "'.join(CN_MONTHS + JP_MONTHS) + '"]),\n'
table = re.sub(r'    \("yf", &\[[^\]]*\]\),\n', merged_yf, table)

# 替换 SYMBOL_TABLE 段（const 行到 ]; 行）
pattern = re.compile(
    r'(const SYMBOL_TABLE: &\[\(&str, &\[&str\]\)\] = &\[\n).*?(\];)',
    re.DOTALL,
)
assert pattern.search(rs), "SYMBOL_TABLE 未找到"
rs = pattern.sub(lambda m: m.group(1) + table + "\n" + m.group(2), rs, count=1)

io.open(SYMBOL_RS, "w", encoding="utf-8").write(rs)
print("替换完成。yf 行：")
for ln in rs.split("\n"):
    if ln.startswith('    ("yf"'):
        print(ln[:200])
