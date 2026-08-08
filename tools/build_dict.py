#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
泰深输入法 — 吸收开源竞品词库（雾凇拼音 rime-ice，BSD 兼容）
=============================================================
数据源：github.com/iDvel/rime-ice/cn_dicts
  - 8105.dict.yaml：通用规范汉字表 8105 字（全量吸收，单字覆盖）
  - base.dict.yaml：基础词库 54 万条（按频率取 top N 吸收，控制体积）
  - ext.dict.yaml：扩展词库 33.9 万条（V0.2.16 全量吸收，自带注音）
  - tencent.dict.yaml：腾讯词向量 98 万条（V0.2.16 截取 3-5 字词，pypinyin 自动注音）

合并策略：
  - 与本地已有词条去重（同词取更高频率）
  - 拼音统一去空格（"a a a" → "aaa"）
  - 频率映射：雾凇频率(1~2千万级) 缩放到我们引擎的量级（1~5000）
  - 输出 system_dict.db（覆盖）

用法：python tools/build_dict.py
"""
import os
import sqlite3
import sys
import urllib.request

try:
    from pypinyin import pinyin, Style
except ImportError:
    print("需要 pypinyin: pip install pypinyin")
    sys.exit(1)

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT_DB = os.path.join(ROOT, "resources", "system_dict.db")
RAW_DIR = os.path.join(ROOT, "resources", "rime_ice")

# 吸收配置
# TOP_N_BASE：base 词库截取量。⚠️ 坑（V0.3.x2 修复）：雾凇 base 中 freq=9999
#（顶格"常用词"档，含大量常用双字词如"社会/上海/时候"）共 82911 条，
# 旧值 50000 按 freq 降序截取时（截断点 freq≥18020）把它们全部砍掉——
# 导致"社会"（freq=9999）等最常用词从词库消失。120000 覆盖 9999 档全部。
TOP_N_BASE = 120000        # base 词库取高频前 12 万条（覆盖 9999 常用词档）
MAX_FREQ = 5000           # 引擎频率上限
UA = {"User-Agent": "Mozilla/5.0"}
# V0.2.16：代理（raw.githubusercontent.com 直连 TLS 失败时走本地代理）
# 从环境变量读取，避免在仓库中硬编码本机代理地址；未设置则直连
PROXY = os.environ.get("HTTP_PROXY") or os.environ.get("HTTPS_PROXY") or ""
# V0.2.16：tencent 词库截取参数（全量 98 万 → 3 字词 ~22 万，控制内存）
# 实测 3-5 字纯汉字词 85 万，全收会到 125 万条（引擎全量加载过重）。
# 3 字词是长词主力（人名/地名/机构），4 字+ 后续按需补充。
TENCENT_MIN_LEN = 3
TENCENT_MAX_LEN = 3


def download(url: str, dest: str) -> None:
    """下载文件（已存在则跳过）。raw.githubusercontent.com 直连 TLS 失败 → 走代理。"""
    if os.path.exists(dest):
        print(f"  已存在: {dest}")
        return
    print(f"  下载: {url}")
    req = urllib.request.Request(url, headers=UA)
    if PROXY:
        handler = urllib.request.ProxyHandler({"http": PROXY, "https": PROXY})
        opener = urllib.request.build_opener(handler)
    else:
        opener = urllib.request.build_opener()
    try:
        with opener.open(req, timeout=60) as resp:
            data = resp.read()
    except Exception as e:
        # 代理失败 → 直连重试（本机网络可能已通）
        print(f"  代理下载失败({e})，尝试直连...")
        with urllib.request.urlopen(req, timeout=30) as resp:
            data = resp.read()
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    with open(dest, "wb") as f:
        f.write(data)
    print(f"  完成: {len(data)} bytes")


def parse_rime_dict(path: str) -> list[tuple[str, str, int]]:
    """解析 Rime .dict.yaml → [(word, pinyin_nospace, freq)]"""
    entries = []
    with open(path, "r", encoding="utf-8", errors="replace") as f:
        in_data = False
        for line in f:
            line = line.rstrip("\n")
            if line == "---":
                in_data = True
                continue
            if not in_data or not line or line.startswith("#"):
                continue
            parts = line.split("\t")
            if len(parts) != 3:
                continue
            word, pys, freq_str = parts
            if not word or not pys or not freq_str.strip().isdigit():
                continue
            # 拼音去空格（多音字注释等异常行跳过）
            py_joined = pys.replace(" ", "")
            if not py_joined or any(not c.isalpha() for c in py_joined):
                continue
            freq = int(freq_str)
            entries.append((word, py_joined, freq))
    return entries


def parse_tencent_dict(path: str) -> list[tuple[str, str, int]]:
    """解析腾讯词向量 .dict.yaml → [(word, pinyin_nospace, freq=100)]。

    tencent.dict.yaml 只有 text + weight 两列（无拼音），
    需 pypinyin 自动注音。截取 TENCENT_MIN_LEN..MAX_LEN 字词。
    """
    entries = []
    words = []
    with open(path, "r", encoding="utf-8", errors="replace") as f:
        in_data = False
        for line in f:
            line = line.rstrip("\n")
            if line == "---":
                in_data = True
                continue
            if not in_data or not line or line.startswith("#"):
                continue
            parts = line.split("\t")
            if len(parts) != 2:
                continue
            word, freq_str = parts
            if not word or not freq_str.strip().isdigit():
                continue
            # 截取 3-5 字纯汉字词（跳过含数字/字母/标点的行）
            if not (TENCENT_MIN_LEN <= len(word) <= TENCENT_MAX_LEN):
                continue
            if not all("\u4e00" <= ch <= "\u9fff" for ch in word):
                continue
            words.append(word)

    print(f"    tencent 截取后 {len(words)} 词，pypinyin 批量注音...")
    # pypinyin 批量注音（heteronym=False 取第一读音）
    # ⚠️ 坑：pypinyin 对 list 输入返回【扁平】列表（每字一个 [pinyin]），
    # 如 pinyin(['一万一']) → [['yi'],['wan'],['yi']]。
    # 旧实现 zip(chunk, py_list) 错位 + p[0] 取首字符 → 22 万三字词注音退化为单字母（V0.3.x 修复）。
    # 正确做法：按词逐字切分扁平结果，每词取 n=len(word) 个元素。
    batch = 2000
    for i in range(0, len(words), batch):
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
            entries.append((word, py_joined, 100))
        if (i // batch) % 20 == 19:
            print(f"      注音进度: {i + len(chunk)}/{len(words)}")
    return entries


def scale_freq(freq: int) -> int:
    """雾凇频率(1~2千万) → 引擎量级(1~5000)。对数压缩。"""
    if freq <= 0:
        return 1
    import math
    v = int(5000 * math.log10(freq) / math.log10(20_000_000))
    return max(1, min(MAX_FREQ, v))


def gb2312_level1_chars():
    """GB2312 一级汉字：区 16-55。共 3755 字（本地兜底单字）。"""
    chars = []
    for qu in range(16, 56):
        for wei in range(1, 95):
            try:
                ch = bytes([qu + 0xA0, wei + 0xA0]).decode("gb2312")
                chars.append(ch)
            except Exception:
                pass
    return chars


def build():
    os.makedirs(RAW_DIR, exist_ok=True)
    url_base = "https://raw.githubusercontent.com/iDvel/rime-ice/main/cn_dicts"

    # 1. 下载
    print("[1/5] 下载雾凇词库...")
    download(f"{url_base}/8105.dict.yaml", os.path.join(RAW_DIR, "8105.dict.yaml"))
    download(f"{url_base}/base.dict.yaml", os.path.join(RAW_DIR, "base.dict.yaml"))
    download(f"{url_base}/ext.dict.yaml", os.path.join(RAW_DIR, "ext.dict.yaml"))
    download(f"{url_base}/tencent.dict.yaml", os.path.join(RAW_DIR, "tencent.dict.yaml"))

    # 2. 解析合并
    print("[2/5] 解析词库...")
    all_entries: dict[tuple[str, str], int] = {}

    def merge(entries: list[tuple[str, str, int]]):
        for word, py, freq in entries:
            key = (word, py)
            # 同词同音取更高频率
            if key not in all_entries or freq > all_entries[key]:
                all_entries[key] = freq

    # 8105 常用字全量
    print("  解析 8105 常用字...")
    e8105 = parse_rime_dict(os.path.join(RAW_DIR, "8105.dict.yaml"))
    print(f"    8105: {len(e8105)} 条")
    merge(e8105)

    # base 词库取高频 top N
    print(f"  解析 base 词库（top {TOP_N_BASE}）...")
    ebase = parse_rime_dict(os.path.join(RAW_DIR, "base.dict.yaml"))
    print(f"    base: {len(ebase)} 条，截取高频...")
    ebase.sort(key=lambda x: -x[2])
    merge(ebase[:TOP_N_BASE])

    # ext 扩展词库全量（V0.2.16）
    print("  解析 ext 扩展词库（全量）...")
    eext = parse_rime_dict(os.path.join(RAW_DIR, "ext.dict.yaml"))
    print(f"    ext: {len(eext)} 条")
    merge(eext)

    # tencent 腾讯词向量（V0.2.16，截取 3 字词 + 自动注音）
    print("  解析 tencent 腾讯词向量（截取 3 字 + 自动注音）...")
    etencent = parse_tencent_dict(os.path.join(RAW_DIR, "tencent.dict.yaml"))
    print(f"    tencent: {len(etencent)} 条")
    merge(etencent)

    # 本地 GB2312 单字兜底（8105 已覆盖，仅补漏）
    print("  本地 GB2312 单字兜底...")
    for idx, ch in enumerate(gb2312_level1_chars()):
        if ch in all_entries:
            continue
        py_list = pinyin(ch, style=Style.NORMAL, heteronym=False)
        py = py_list[0][0] if py_list else ""
        if py:
            freq = max(100, 4000 - (idx // 94 - 16) * 80)
            all_entries[(ch, py)] = freq

    # 3. 写库
    print(f"[3/5] 写入 SQLite（共 {len(all_entries)} 条）...")
    if os.path.exists(OUT_DB):
        os.remove(OUT_DB)
    conn = sqlite3.connect(OUT_DB)
    conn.execute("CREATE TABLE system_dict (pinyin TEXT, word TEXT, frequency INTEGER)")
    rows = [(py, word, scale_freq(freq)) for (word, py), freq in all_entries.items()]
    conn.executemany(
        "INSERT INTO system_dict (pinyin, word, frequency) VALUES (?, ?, ?)", rows
    )
    conn.execute("CREATE INDEX idx_pinyin ON system_dict(pinyin, frequency DESC)")
    conn.commit()
    conn.close()

    # 4. 验证
    print("[4/5] 验证...")
    conn = sqlite3.connect(OUT_DB)
    total = conn.execute("SELECT COUNT(*) FROM system_dict").fetchone()[0]
    multi = conn.execute(
        "SELECT COUNT(*) FROM system_dict WHERE LENGTH(word)>1"
    ).fetchone()[0]
    print(f"  总词条: {total}（单字 {total-multi}，多字 {multi}）")
    for py in ["zhong", "zhongguo", "nihao", "ai", "shijie", "rengongzhineng", "shehui", "shihou"]:
        rows = conn.execute(
            "SELECT word FROM system_dict WHERE pinyin=? ORDER BY frequency DESC LIMIT 5",
            (py,),
        ).fetchall()
        print(f"  {py}: {[r[0] for r in rows]}")
    conn.close()
    print("完成！")


if __name__ == "__main__":
    build()
