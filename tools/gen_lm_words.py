"""生成 `resources/lm/lm_words.json`（词 → 语言模型 ID）。

用法：
    python tools/gen_lm_words.py --lex <词库文件路径>

前置：指定一个本机资源文件（不入库，路径由调用方给出）。
产物：`resources/lm/lm_words.json`（约 10 MB，已 gitignore，不入库）。
格式说明见本机笔记，不入库。
"""
import argparse
import json
import struct
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "resources" / "lm" / "lm_words.json"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--lex", required=True, help="本机词库文件路径")
    args = ap.parse_args()
    lex = Path(args.lex)
    if not lex.exists():
        print(f"找不到词库文件: {lex}", file=sys.stderr)
        return 1

    buf = lex.read_bytes()
    off = {i: struct.unpack_from("<I", buf, 0xC0 + i * 4)[0] for i in range(8)}

    char_count = struct.unpack_from("<I", buf, off[0])[0]
    base = off[0] + 4
    chars = [struct.unpack_from("<H", buf, base + i * 8)[0] for i in range(char_count - 1)]

    pool = buf[off[2]:off[3]]
    offsets = sorted(struct.unpack_from(f"<{(off[5] - off[4]) // 4}I", buf, off[4]))

    words = {}
    for i in range(len(offsets) - 1):
        rec = pool[offsets[i]:offsets[i + 1]]
        if len(rec) < 6:
            continue
        wid = struct.unpack_from("<I", rec, 0)[0] & 0xFFFFF
        ids = struct.unpack_from("<" + "H" * ((len(rec) - 4) // 2), rec, 4)
        text = "".join(chr(chars[c]) for c in ids if 0 < c < len(chars))
        if text:
            words.setdefault(text, wid)

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(words, ensure_ascii=False), encoding="utf-8")
    print(f"写入 {OUT}：{len(words)} 词，{OUT.stat().st_size / 1024 / 1024:.1f} MB")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
