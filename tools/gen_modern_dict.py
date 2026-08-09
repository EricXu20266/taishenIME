#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
泰深输入法 — LLM 现代词表生成工具
=================================
读 tools/gen_modern_words.txt（纯词，LLM 生成）→ pypinyin 转拼音 →
与现有词库（system_dict.db + domains/*.txt）去重 →
输出 resources/domains/modern.txt（词 拼音 每行一个）。

用法：python tools/gen_modern_dict.py
"""
import os
import re
import sqlite3
import sys

from pypinyin import Style, lazy_pinyin

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WORDS_SRC = os.path.join(ROOT, "tools", "gen_modern_words.txt")
DOMAINS_DIR = os.path.join(ROOT, "resources", "domains")
OUT = os.path.join(DOMAINS_DIR, "modern.txt")
SYSTEM_DB = os.path.join(ROOT, "resources", "system_dict.db")

CJK_RE = re.compile(r"[\u4e00-\u9fff]")


def load_system_words(db_path: str) -> set:
    """读 system_dict.db 全部词（jieba 基底，38 万词），用于去重。"""
    if not os.path.exists(db_path):
        print(f"[WARN] system_dict.db 不存在: {db_path}，跳过 system 去重")
        return set()
    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    cur = conn.cursor()
    words = {r[0] for r in cur.execute("SELECT word FROM system_dict")}
    conn.close()
    print(f"system_dict.db 已有词: {len(words)}")
    return words


def load_domain_words() -> set:
    """读 domains/*.txt 全部词。"""
    words = set()
    for fn in os.listdir(DOMAINS_DIR):
        if not fn.endswith(".txt"):
            continue
        path = os.path.join(DOMAINS_DIR, fn)
        with open(path, encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith("#"):
                    continue
                parts = line.split()
                if parts:
                    words.add(parts[0])
    return words


def pinyin_for(word: str) -> str:
    """pypinyin 全拼（NORMAL 风格去空格，中英混词保留 ASCII 原样）。"""
    syls = lazy_pinyin(word, style=Style.NORMAL, errors="default")
    return "".join(syls)


def main():
    if not os.path.exists(WORDS_SRC):
        print(f"错误：词表不存在 {WORDS_SRC}")
        sys.exit(1)

    # 现有词集合（去重用）
    system_words = load_system_words(SYSTEM_DB)
    domain_words = load_domain_words()
    print(f"domains/*.txt 已有词: {len(domain_words)}")

    # 读取 LLM 词表
    candidates = []
    with open(WORDS_SRC, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            candidates.append(line)
    print(f"LLM 词表候选: {len(candidates)} 词")

    kept, dup, skipped = [], 0, 0
    seen = set()
    for word in candidates:
        if not CJK_RE.search(word):
            skipped += 1  # 纯英文（如 FIRE）——引擎走英文直出路径，不入领域库
            continue
        if word in system_words or word in domain_words:
            dup += 1  # 已有词库覆盖，不重复加
            continue
        py = pinyin_for(word)
        if not py or not py.isascii():
            skipped += 1
            continue
        py = py.lower()
        if word in seen:
            continue
        seen.add(word)
        kept.append((word, py))

    kept.sort()
    with open(OUT, "w", encoding="utf-8") as f:
        f.write("# 泰深输入法专业词库 — modern\n")
        f.write("# 格式：词 拼音（每行一个，制表符分隔）\n")
        f.write("# 来源：AI 生成现代高频词表（平台/科技/网络用语/新消费），pypinyin 注音\n")
        f.write("# 生成: 2026-08-09\n")
        f.write("\n")
        for word, py in kept:
            f.write(f"{word}\t{py}\n")

    print(f"\n结果：新词 {len(kept)} | 已有去重 {dup} | 跳过(纯英文/异常) {skipped}")
    print(f"输出: {OUT}")
    if kept:
        for w, p in kept[:15]:
            print(f"  {w}\t{p}")
        print("  ...")


if __name__ == "__main__":
    main()
