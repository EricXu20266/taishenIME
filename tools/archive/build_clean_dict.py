#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
泰深输入法 — 干净许可系统词库构建（V0.3.7 合规换源）
=============================================================
数据源（许可全部可商业使用）：
  - jieba dict.txt（MIT License，GitHub fxsjy/jieba）
      34.9 万词条，含词频；本项目仅提取纯汉字词 + pypinyin 注音
  - pypinyin（MIT License）汉字→拼音注音

对比旧来源（已废弃）：
  - rime-ice（GPL-3.0）——与项目 MIT 许可冲突，不再使用
  - 搜狗细胞词库（用户协议禁止提取再分发）——不再使用

用法：python tools/build_clean_dict.py
输出：tools/tmp_clean/clean_dict.txt（pinyin\\tword\\tfreq 三元组）
      然后 build_system_dict.py 入库 system_dict.db
"""
import math
import os
import re
import sys
import urllib.request

try:
    from pypinyin import pinyin, Style
except ImportError:
    print("需要 pypinyin: pip install pypinyin")
    sys.exit(1)

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
JIEBA_DICT = os.path.join(ROOT, "tools", "tmp_clean", "jieba_dict.txt")
JIEBA_URL = "https://raw.githubusercontent.com/fxsjy/jieba/master/jieba/dict.txt"
OUT_TXT = os.path.join(ROOT, "tools", "tmp_clean", "clean_dict.txt")

# 引擎参数（与旧 build_dict.py 保持一致）
MAX_FREQ = 5000          # 引擎频率上限
MAX_WORD_LEN = 6         # 最长词（jieba 含更长词条，输入法用不到）
MIN_WORD_LEN = 1

# 词频压缩：jieba 词频(1~88万) 对数映射 → 引擎量级(1~5000)
JIEBA_MAX_FREQ = 883634  # jieba dict.txt 实测最大值


def download_jieba() -> None:
    """下载 jieba dict.txt（MIT，幂等：已存在跳过）。"""
    if os.path.exists(JIEBA_DICT):
        print(f"  已存在: {JIEBA_DICT}")
        return
    print(f"  下载: {JIEBA_URL}")
    req = urllib.request.Request(JIEBA_URL, headers={"User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req, timeout=60) as resp:
        data = resp.read()
    os.makedirs(os.path.dirname(JIEBA_DICT), exist_ok=True)
    with open(JIEBA_DICT, "wb") as f:
        f.write(data)
    print(f"  完成: {len(data)} bytes")


def parse_jieba() -> list[tuple[str, int]]:
    """解析 jieba dict.txt → [(word, freq)]。仅保留纯汉字 1..MAX_WORD_LEN 词。"""
    entries = []
    hanzi_re = re.compile(r"^[\u4e00-\u9fff]{1,%d}$" % MAX_WORD_LEN)
    with open(JIEBA_DICT, "r", encoding="utf-8", errors="replace") as f:
        for line in f:
            parts = line.rstrip("\n").split(" ")
            if len(parts) < 2:
                continue
            word, freq_str = parts[0], parts[1]
            if not hanzi_re.match(word):
                continue
            if not freq_str.isdigit():
                continue
            freq = int(freq_str)
            if freq <= 0:
                freq = 1
            entries.append((word, freq))
    return entries


def annotate(entries: list[tuple[str, int]]) -> list[tuple[str, str, int]]:
    """pypinyin 批量注音（Style.NORMAL 无声调，去空格拼接）。

    ⚠️ 坑（V0.3.x 修复过）：pypinyin 对 list 输入返回扁平列表
    [['yi'],['wan'],['yi']]，必须按 len(word) 逐词切分，不能 zip 错位。
    """
    words = [w for w, _ in entries]
    result = []
    batch = 2000
    total = len(words)
    for i in range(0, total, batch):
        chunk = words[i : i + batch]
        py_flat = pinyin(chunk, style=Style.NORMAL, heteronym=False)
        idx = 0
        for word in chunk:
            n = len(word)
            syls = py_flat[idx : idx + n]
            idx += n
            py_joined = "".join(s[0] for s in syls if s)
            if not py_joined or any(not c.isalpha() for c in py_joined):
                continue
            result.append((word, py_joined, 0))  # freq 稍后填
        if ((i // batch) % 20) == 19:
            print(f"      注音进度: {min(i + len(chunk), total)}/{total}")
    print(f"  注音完成: {len(result)}/{total} 词")
    return result


def scale_freq(freq: int) -> int:
    """jieba 词频(1~88万) → 引擎量级(1~5000)。对数压缩。"""
    if freq <= 0:
        return 1
    v = int(MAX_FREQ * math.log10(freq) / math.log10(JIEBA_MAX_FREQ))
    return max(1, min(MAX_FREQ, v))


def main() -> None:
    download_jieba()
    print("解析 jieba 词典...")
    entries = parse_jieba()
    print(f"  纯汉字词 {len(entries)} 条")
    print("pypinyin 批量注音...")
    annotated = annotate(entries)
    print("词频压缩 + 写入...")
    os.makedirs(os.path.dirname(OUT_TXT), exist_ok=True)
    freq_map = {w: fr for w, fr in entries}
    with open(OUT_TXT, "w", encoding="utf-8") as f:
        for word, py, _ in annotated:
            freq = scale_freq(freq_map[word])
            f.write(f"{py}\t{word}\t{freq}\n")
    print(f"完成: {OUT_TXT}")

    # 统计
    words = [w for w, _, _ in annotated]
    print(f"  总词数: {len(words)}")
    single = sum(1 for w in words if len(w) == 1)
    print(f"  单字: {single}, 双字: {sum(1 for w in words if len(w) == 2)}, "
          f"3字+: {sum(1 for w in words if len(w) >= 3)}")


if __name__ == "__main__":
    main()
