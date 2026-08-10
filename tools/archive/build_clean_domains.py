#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
泰深输入法 — 干净许可领域词库构建（V0.3.7 合规换源）
=============================================================
数据源：中文维基百科（CC BY-SA 4.0）
  按领域分类递归提取词条标题 → pypinyin 注音 → domains/*.txt

对比旧来源（已废弃）：
  - 搜狗细胞词库（用户协议禁止提取再分发）——不再使用

许可说明：维基百科内容以 CC BY-SA 4.0 授权，允许商业使用，
要求署名——因此输出文件头保留来源声明（与搜狗来源相反，这是
合法使用要求的署名）。

用法：python tools/build_clean_domains.py [领域名...]
不传参数 = 构建全部领域。
"""
import os
import re
import sys
import time
import urllib.parse
import urllib.request
import ssl

try:
    from pypinyin import pinyin, Style
except ImportError:
    print("需要 pypinyin: pip install pypinyin")
    sys.exit(1)

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOMAINS_DIR = os.path.join(ROOT, "resources", "domains")

UA = "TaishenIME-DictBuilder/1.0 (https://github.com/EricXu20266/taishenIME)"
API = "https://zh.wikipedia.org/w/api.php"
PROXY = os.environ.get("HTTP_PROXY") or os.environ.get("HTTPS_PROXY") or ""
MAX_DEPTH = 2          # 子分类递归深度（主分类=0）
MAX_WORDS = 25000      # 每领域词条上限（防膨胀）
REQUEST_GAP = 0.15     # API 限流（秒）
MAX_WORD_LEN = 6

# 领域 → 维基百科分类（可多个；子分类自动递归）
# 注：网络流行词不适合维基（词条多为外文/数字，纯汉字少），未纳入；
#     网络/成语基础覆盖由系统词库（jieba）提供。
DOMAINS = {
    "computer":    ["Category:计算机科学"],
    "math":        ["Category:数学"],
    "physics":     ["Category:物理学"],
    "chemistry":   ["Category:化学"],
    "biology":     ["Category:生物学"],
    "geography":   ["Category:地理学", "Category:地质学"],
    "astronomy":   ["Category:天文学"],
    "meteorology": ["Category:气象学"],
    "idiom":       ["Category:成语"],
}

_ctx = ssl._create_unverified_context()


def _opener():
    handlers = [urllib.request.HTTPSHandler(context=_ctx)]
    if PROXY:
        handlers.append(urllib.request.ProxyHandler({"http": PROXY, "https": PROXY}))
    return urllib.request.build_opener(*handlers)


def api_call(params: dict) -> dict:
    """MediaWiki API 请求（带 UA + 分页 continue 自动翻页 + 失败重试）。"""
    import json
    url = API + "?" + urllib.parse.urlencode(params)
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    opener = _opener()
    members = []
    while True:
        data = _request_with_retry(req, opener)
        for m in data.get("query", {}).get("categorymembers", []):
            members.append(m)
        cont = data.get("continue", {})
        if "cmcontinue" in cont:
            params = dict(params)
            params.update(cont)
            url = API + "?" + urllib.parse.urlencode(params)
            req = urllib.request.Request(url, headers={"User-Agent": UA})
        else:
            break
        time.sleep(REQUEST_GAP)
    return members


def _request_with_retry(req, opener, tries: int = 3) -> dict:
    """请求 + 重试（SSL EOF / 429 等暂时性错误）。"""
    import json
    last_err = None
    for attempt in range(tries):
        try:
            with opener.open(req, timeout=30) as r:
                return json.load(r)
        except Exception as e:
            last_err = e
            time.sleep(1.0 * (attempt + 1))
    raise last_err


def collect_category(cat: str, depth: int, visited: set, out: set) -> None:
    """递归收集分类下页面标题（深度限制 + visited 防环）。"""
    if depth > MAX_DEPTH or cat in visited:
        return
    visited.add(cat)
    try:
        members = api_call({
            "action": "query",
            "list": "categorymembers",
            "cmtitle": cat,
            "cmlimit": "500",
            "cmnamespace": "0|14",  # 0=页面 14=子分类
            "format": "json",
        })
    except Exception as e:
        print(f"    [warn] {cat} 请求失败: {e}")
        return
    for m in members:
        ns = m.get("ns")
        title = m.get("title", "")
        if ns == 14:  # 子分类
            collect_category(title, depth + 1, visited, out)
        elif ns == 0:  # 页面
            out.add(title)
    time.sleep(REQUEST_GAP)


def clean_title(title: str) -> str | None:
    """清洗词条标题 → 纯汉字词（去括号消歧义、去标点、长度限制）。"""
    # 去括号后缀："函数（数学）" → "函数"
    t = title.split("（")[0].split("(")[0].strip()
    if not re.fullmatch(r"[\u4e00-\u9fff]{1,%d}" % MAX_WORD_LEN, t):
        return None
    return t


def annotate_words(words: list[str]) -> dict[str, str]:
    """pypinyin 批量注音 → {word: pinyin_nospace}（注音失败跳过）。"""
    result = {}
    batch = 1000
    for i in range(0, len(words), batch):
        chunk = words[i : i + batch]
        py_flat = pinyin(chunk, style=Style.NORMAL, heteronym=False)
        idx = 0
        for word in chunk:
            n = len(word)
            syls = py_flat[idx : idx + n]
            idx += n
            py_joined = "".join(s[0] for s in syls if s)
            if py_joined and py_joined.isascii() and py_joined.islower():
                result[word] = py_joined
        if (i // batch) % 10 == 9:
            print(f"      注音进度: {min(i + len(chunk), len(words))}/{len(words)}")
    return result


def build_domain(name: str, cats: list[str]) -> None:
    print(f"== 领域 {name}（{cats}）==")
    visited: set = set()
    collected: set = set()
    for cat in cats:
        collect_category(cat, 0, visited, collected)
    print(f"  原始词条 {len(collected)}，清洗去重...")
    words = []
    seen = set()
    for t in collected:
        w = clean_title(t)
        if w and w not in seen:
            seen.add(w)
            words.append(w)
    if len(words) > MAX_WORDS:
        words = words[:MAX_WORDS]
    print(f"  纯汉字词 {len(words)}，pypinyin 注音...")
    py_map = annotate_words(words)
    print(f"  注音成功 {len(py_map)}")

    os.makedirs(DOMAINS_DIR, exist_ok=True)
    out_path = os.path.join(DOMAINS_DIR, f"{name}.txt")
    with open(out_path, "w", encoding="utf-8") as f:
        f.write(f"# 泰深输入法专业词库 — {name}\n")
        f.write("# 格式：词 拼音（每行一个，空格或制表符分隔）\n")
        f.write("# 来源：中文维基百科（CC BY-SA 4.0，https://zh.wikipedia.org）\n")
        f.write("\n")
        for w in sorted(py_map.keys()):
            f.write(f"{w}\t{py_map[w]}\n")
    print(f"  完成: {out_path}（{len(py_map)} 词条）\n")


def main() -> None:
    targets = sys.argv[1:] or list(DOMAINS.keys())
    for name in targets:
        if name not in DOMAINS:
            print(f"未知领域: {name}（可选: {', '.join(DOMAINS)}）")
            continue
        build_domain(name, DOMAINS[name])


if __name__ == "__main__":
    main()
