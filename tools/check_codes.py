# -*- coding: utf-8 -*-
import io
import re

rs = io.open(r"engine\src\symbol.rs", encoding="utf-8").read()
codes = re.findall(r'\("([a-z0-9]+)",\s*&\[', rs) + re.findall(r'\("([a-z0-9]+)",\s*$', rs, re.M)
codes = list(dict.fromkeys(codes))
print("分类数:", len(codes))
for probe in ["no", "ha", "ni", "wo", "wa", "ga", "ka", "de", "to", "o", "ya", "ru"]:
    print(probe, "冲突" if probe in codes else "可用")
