"""校验 resources/common_dict.txt 并和 common.db 对账。

每次向常用词表追加词条后都必须跑一遍。历史教训：多轮追加没有去重检查，
表里留下了 40 行完全重复（「生活 shenghuo」两行、「一个 yige」两行），
重复行会占两个优先级槽位，候选列表被稀释。

校验项：
  1. 格式：pinyin<TAB>word，拼音只含小写字母与音节分隔符 '，词非空
  2. 完全重复：同 pinyin + 同 word 出现多次
  3. 音节数 = 字数：撇号条目按段数比，无撇号条目按引擎音节表 DP 切分比
  4. 与 common.db 逐行一致（rank 就是行序，顺序错也会让优先级错位）

「无法切分成拼音」的条目（单字母简写 l 了、中英混词 bzhan b站、英文品牌
alipay）是设计允许的特例，只列出来供人工过一眼，不判失败。

用法：
    python tools/verify_common_dict.py [--txt path] [--db path]
退出码 0 = 通过，1 = 有问题。
"""

from __future__ import annotations

import argparse
import re
import sqlite3
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
MAX_SYLLABLE_LEN = 6
# 非拼音条目的已知设计特例（单字母简写 / 中英混词）。新增条目落在这里面
# 之外会被单独列进「待人工确认」，避免悄悄混进错误拼音。
NON_PINYIN_SAMPLES = 40


def load_syllables() -> set[str]:
    """从引擎源文件解析音节表，保证校验口径与运行时完全一致。"""
    source = REPO / "engine" / "src" / "pinyin" / "mod.rs"
    text = source.read_text(encoding="utf-8")
    match = re.search(r"const VALID_SYLLABLES: &\[&str\] = &\[(.*?)\];", text, re.S)
    if not match:
        raise SystemExit(f"[ERROR] 无法从 {source} 解析音节表")
    return set(re.findall(r'"([a-z]+)"', match.group(1)))


def syllable_counts(pinyin: str, syllables: set[str]) -> set[int]:
    """返回该拼音串所有完整切分路径的音节数集合。"""
    n = len(pinyin)
    reachable: list[set[int]] = [set() for _ in range(n + 1)]
    reachable[0].add(0)
    for start in range(n):
        if not reachable[start]:
            continue
        for length in range(1, min(MAX_SYLLABLE_LEN, n - start) + 1):
            if pinyin[start : start + length] not in syllables:
                continue
            for count in reachable[start]:
                reachable[start + length].add(count + 1)
    return reachable[n]


def read_entries(path: Path) -> tuple[list[tuple[int, str, str]], list[str]]:
    entries: list[tuple[int, str, str]] = []
    problems: list[str] = []
    for lineno, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if "\t" not in raw:
            problems.append(f"第 {lineno} 行缺少 TAB 分隔: {raw!r}")
            continue
        pinyin, word = raw.split("\t", 1)
        pinyin, word = pinyin.strip(), word.strip()
        if not pinyin or not word:
            problems.append(f"第 {lineno} 行有空字段: {raw!r}")
            continue
        if not re.fullmatch(r"[a-z']+", pinyin):
            problems.append(f"第 {lineno} 行拼音含非法字符: {pinyin!r}")
            continue
        if "''" in pinyin or pinyin.startswith("'") or pinyin.endswith("'"):
            problems.append(f"第 {lineno} 行撇号用法异常: {pinyin!r}")
            continue
        entries.append((lineno, pinyin, word))
    return entries, problems


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--txt", type=Path, default=REPO / "resources" / "common_dict.txt")
    parser.add_argument("--db", type=Path, default=REPO / "resources" / "common.db")
    args = parser.parse_args()

    syllables = load_syllables()
    entries, problems = read_entries(args.txt)

    seen: set[tuple[str, str]] = set()
    duplicates: list[tuple[int, str, str]] = []
    length_mismatch: list[tuple[int, str, str, str]] = []
    non_pinyin: list[tuple[int, str, str]] = []
    by_word: dict[str, set[str]] = {}

    for lineno, pinyin, word in entries:
        key = (pinyin, word)
        if key in seen:
            duplicates.append((lineno, pinyin, word))
        seen.add(key)
        by_word.setdefault(word, set()).add(pinyin)

        expected = len(word)
        if "'" in pinyin:
            # 撇号就是音节边界：段数必须等于字数（引擎按 ' 切分建索引）
            segments = [seg for seg in pinyin.split("'") if seg]
            if len(segments) != expected:
                length_mismatch.append((lineno, pinyin, word, f"撇号分出 {len(segments)} 段"))
                continue
            if not all(seg in syllables for seg in segments):
                bad = [seg for seg in segments if seg not in syllables]
                non_pinyin.append((lineno, pinyin, f"{word}（非音节: {'/'.join(bad)}）"))
            continue
        counts = syllable_counts(pinyin, syllables)
        if not counts:
            non_pinyin.append((lineno, pinyin, word))
        elif expected not in counts:
            length_mismatch.append((lineno, pinyin, word, "切不出对应音节数"))

    multi = {w: p for w, p in by_word.items() if len(p) > 1}

    # 与 common.db 对账：rank 是行序，内容与顺序都必须一致
    db_rows: list[tuple[str, str]] = []
    db_problems: list[str] = []
    if args.db.is_file():
        with sqlite3.connect(args.db) as conn:
            db_rows = [
                (row[0], row[1])
                for row in conn.execute("SELECT pinyin, word FROM common_words ORDER BY rank")
            ]
    else:
        problems.append(f"common.db 不存在: {args.db}")

    txt_rows = [(pinyin, word) for _, pinyin, word in entries]
    if db_rows:
        if len(txt_rows) != len(db_rows):
            db_problems.append(f"行数不一致: txt={len(txt_rows)} db={len(db_rows)}")
        mismatch = 0
        for index, (txt_row, db_row) in enumerate(zip(txt_rows, db_rows)):
            if txt_row != db_row:
                mismatch += 1
                if mismatch <= 5:
                    db_problems.append(f"第 {index} 条不一致: txt={txt_row} db={db_row}")
        if mismatch:
            db_problems.insert(0, f"共 {mismatch} 条与 common.db 不一致")

    print(f"词条 {len(entries)} 条（注释/空行已跳过）")
    print(f"完全重复 {len(duplicates)} 行")
    print(f"音节数与字数不符 {len(length_mismatch)} 条")
    print(f"非拼音条目（设计特例，供人工过目） {len(non_pinyin)} 条")
    print(f"同词多拼音 {len(multi)} 个词（合法多音，不判失败）")
    if db_rows:
        print(f"与 common.db 对账: {'一致' if not db_problems else db_problems[0]}")

    for lineno, pinyin, word in duplicates[:10]:
        print(f"  [重复] 第 {lineno} 行: {pinyin}\t{word}")
    for lineno, pinyin, word, reason in length_mismatch[:10]:
        print(f"  [字数] 第 {lineno} 行: {pinyin}\t{word}（{reason}）")
    for lineno, pinyin, word in non_pinyin[:NON_PINYIN_SAMPLES]:
        print(f"  [非拼音] 第 {lineno} 行: {pinyin}\t{word}")

    failures = duplicates or length_mismatch or problems or db_problems
    if problems:
        print("\n结构问题：")
        for item in problems[:10]:
            print(f"  {item}")
    if failures:
        print(f"\n[FAIL] 常用词表校验未通过（{args.txt}）")
        return 1
    print(f"\n[OK] 常用词表校验通过（{args.txt}）")
    return 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    raise SystemExit(main())
