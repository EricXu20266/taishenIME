#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
泰深输入法 — 专业词库构建工具（搜狗细胞词库 → 泰深 domain txt）
=============================================================
从搜狗官方细胞词库下载 .scel，解析为"词 拼音"格式，
输出到 resources/domains/<领域>.txt。

用法：python tools/build_domain_dict.py [领域名...]
不传参数 = 构建全部预置领域。

依赖：pip install pypinyin（已有）
"""
import io
import os
import struct
import sys
import urllib.request

# 预置领域：名称 → 搜狗词库详情页 ID（官方推荐）
# ID 从 pinyin.sogou.com/dict/detail/index/<id> 获取
PRESETS = {
    "computer": 15117,   # 计算机词汇大全【官方推荐】10300 词
    "math": 15202,       # 数学词汇大全 15992 词
    "physics": 15203,    # 物理词汇大全 13107 词
    "chemistry": 15205,  # 化学化工词汇大全
    "biology": 15124,    # 生物词汇大全
    "geography": 15208,  # 地理地质词汇大全
    "astronomy": 15204,  # 天文词汇大全
    "meteorology": 15120,  # 气象词汇大全
    "network": 4,        # 网络流行新词【官方推荐】24923 词
    "idiom": 15097,      # 成语俗语【官方推荐】46785 词
}

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOMAINS_DIR = os.path.join(ROOT, "resources", "domains")
TMP_DIR = os.path.join(ROOT, "tools", "tmp_scel")
UA = {"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)"}
PROXY = os.environ.get("HTTP_PROXY") or os.environ.get("HTTPS_PROXY") or ""


def download_scel(detail_id: int, name: str) -> bytes:
    """从搜狗详情页的下载链接下载 scel（download_cell.php?id=xxx&name=xxx）。"""
    from urllib.parse import quote
    dl_url = (
        f"https://pinyin.sogou.com/d/dict/download_cell.php"
        f"?id={detail_id}&name={quote(name)}&f=detail"
    )
    req = urllib.request.Request(dl_url, headers=UA)
    opener = urllib.request.build_opener()
    with opener.open(req, timeout=60) as resp:
        return resp.read()


def parse_scel(data: bytes) -> list[tuple[str, str]]:
    """解析搜狗 scel → [(word, pinyin_nospace)]。参考 scel2txt 开源实现。
    格式：拼音表从 0x1544 开始（0x1540 是 4 字节偏移字段）；
    词组表偏移由文件第 4 字节判断（0x44→0x2628，0x45→0x26c4）；
    拼音索引数是字节数/2（py_idx_count 存的是字节数）。
    """
    # 词组表偏移：根据文件头第 4 字节
    hz_start = 0x2628 if (len(data) > 4 and data[4] == 0x44) else 0x26c4
    # 拼音表：从 0x1544 开始，(py_idx:u16, py_len:u16, py_str:utf16le)
    py_map = {}
    pos = 0x1544
    while pos + 4 <= len(data):
        py_idx, py_len = struct.unpack("<HH", data[pos:pos + 4])
        pos += 4
        if py_len <= 0 or pos + py_len > len(data):
            break
        py_str = data[pos:pos + py_len].decode("utf-16-le", errors="replace")
        pos += py_len
        py_map[py_idx] = py_str
        if py_str == "zuo":
            break
    # 词组表
    words = []
    pos = hz_start
    while pos + 2 <= len(data):
        word_count = struct.unpack("<H", data[pos:pos + 2])[0]
        py_count = struct.unpack("<H", data[pos + 2:pos + 4])[0] // 2
        pos += 4
        py_set = []
        ok = True
        for _ in range(py_count):
            if pos + 2 > len(data):
                ok = False
                break
            idx = struct.unpack("<H", data[pos:pos + 2])[0]
            pos += 2
            py = py_map.get(idx)
            if py is None:
                ok = False
                break
            py_set.append(py)
        if not ok:
            break
        pys = "".join(py_set).replace(" ", "")
        for _ in range(word_count):
            if pos + 2 > len(data):
                ok = False
                break
            wlen = struct.unpack("<H", data[pos:pos + 2])[0]
            pos += 2
            if pos + wlen > len(data):
                ok = False
                break
            word = data[pos:pos + wlen].decode("utf-16-le", errors="replace")
            pos += wlen
            pos += 12  # 跳过 ext_len + ext
            if word and pys and pys.isascii() and pys.isalpha():
                words.append((word, pys))
        if not ok:
            break
    return words


def write_domain(name: str, entries: list[tuple[str, str]]) -> int:
    """写入 resources/domains/<name>.txt，格式"词 拼音"。返回写入词条数。"""
    os.makedirs(DOMAINS_DIR, exist_ok=True)
    out = os.path.join(DOMAINS_DIR, f"{name}.txt")
    # 去重 + 排序（按词）
    seen = set()
    lines = []
    for word, py in entries:
        if (word, py) in seen:
            continue
        seen.add((word, py))
        lines.append(f"{word}\t{py}")
    lines.sort()
    with open(out, "w", encoding="utf-8") as f:
        f.write(f"# 泰深输入法专业词库 — {name}\n")
        f.write("# 格式：词 拼音（每行一个，空格或制表符分隔）\n")
        f.write("# 来源：搜狗细胞词库（官方推荐分类），scel2txt 转换\n")
        f.write("\n")
        for l in lines:
            f.write(l + "\n")
    return len(lines)


def main():
    # 领域 → 中文名（下载 URL 用）
    NAMES = {
        "computer": "计算机词汇大全【官方推荐】",
        "math": "数学词汇大全【官方推荐】",
        "physics": "物理词汇大全【官方推荐】",
        "chemistry": "化学化工词汇大全【官方推荐】",
        "biology": "生物词汇大全【官方推荐】",
        "geography": "地理地质词汇大全【官方推荐】",
        "astronomy": "天文词汇大全【官方推荐】",
        "meteorology": "气象词汇大全【官方推荐】",
        "network": "网络流行新词【官方推荐】",
        "idiom": "成语俗语【官方推荐】",
    }
    targets = sys.argv[1:] if len(sys.argv) > 1 else list(PRESETS.keys())
    for name in targets:
        if name not in PRESETS:
            print(f"[SKIP] 未知领域: {name}")
            continue
        detail_id = PRESETS[name]
        cn_name = NAMES.get(name, name)
        print(f"[{name}] 下载 scel (id={detail_id}, {cn_name})...")
        try:
            data = download_scel(detail_id, cn_name)
        except Exception as e:
            print(f"  [ERROR] 下载失败: {e}")
            continue
        print(f"  scel 大小: {len(data)} bytes")
        entries = parse_scel(data)
        print(f"  解析出 {len(entries)} 词条")
        if not entries:
            print("  [WARN] 无词条，跳过")
            continue
        n = write_domain(name, entries)
        print(f"  写入 resources/domains/{name}.txt ({n} 词)")


if __name__ == "__main__":
    main()
