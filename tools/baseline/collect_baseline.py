#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""泰深输入法基线采集器。

用法（在工作区根目录）：
    python tools/baseline/collect_baseline.py                 # 快速：静态+考试+性能+内存+确定性
    python tools/baseline/collect_baseline.py --with-tests    # 额外跑 cargo test
    python tools/baseline/collect_baseline.py --out docs/baseline/v0.1.91

产物：
    <out>/baseline.json   机读，后续版本用它做 diff
    <out>/baseline.md     人读报告

设计原则：只读，不改任何源码/词库；不触发安装与注册。
"""
import argparse
import hashlib
import json
import os
import re
import sqlite3
import subprocess
import sys
import time

try:
    import psutil
except ImportError:
    psutil = None

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# 体积监控目标
BINARIES = [
    "platform/windows/out/taishen_ime.dll",
    "platform/windows/out/taishen_ime_x86.dll",
    "platform/windows/out/taishen_server.exe",
    "engine/target/release/taishen_engine.dll",
]

# 前 6 个是链接依赖（DLL 名），后 3 个是内嵌标记（crate/库字符串），用于识别「引擎被静态链进来」
DEP_PROBES = ["d2d1.dll", "DWrite.dll", "d3d11.dll", "ws2_32.dll", "winhttp.dll",
              "dwmapi.dll", "reqwest", "hyper", "h2-"]

DICTS = {
    "system_dict.db": "resources/system_dict.db",
    "domains.db": "resources/domains/domains.db",
    "common.db": "resources/common.db",
    "bigram.db": "resources/bigram.db",
}


def run(cmd, timeout=600, env=None):
    e = dict(os.environ)
    if env:
        e.update(env)
    try:
        p = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True,
                           encoding="utf-8", errors="ignore", timeout=timeout, env=e)
        return {"rc": p.returncode, "out": p.stdout or "", "err": p.stderr or ""}
    except Exception as ex:
        return {"rc": -1, "out": "", "err": str(ex)}


def read_version():
    v = {"cargo": None, "git": None, "dict": None}
    try:
        with open(os.path.join(ROOT, "engine/Cargo.toml"), encoding="utf-8") as f:
            m = re.search(r'^version\s*=\s*"(.+)"', f.read(), re.M)
            v["cargo"] = m.group(1) if m else None
    except Exception:
        pass
    r = run(["git", "rev-parse", "--short", "HEAD"], timeout=30)
    v["git"] = r["out"].strip() or None
    r = run(["git", "status", "--porcelain"], timeout=30)
    v["dirty"] = bool(r["out"].strip())
    try:
        with open(os.path.join(ROOT, "resources/VERSION.json"), encoding="utf-8") as f:
            v["dict"] = json.load(f)
    except Exception:
        pass
    return v


def collect_binaries():
    out = []
    for rel in BINARIES:
        p = os.path.join(ROOT, rel)
        if not os.path.exists(p):
            out.append({"path": rel, "missing": True})
            continue
        raw = open(p, "rb").read()
        # 依赖用 ASCII 串扫描（等价 dumpbin /dependents 的粗筛）
        text = raw.decode("latin-1", errors="ignore")
        deps = [d for d in DEP_PROBES if d.lower() in text.lower()]
        out.append({
            "path": rel,
            "bytes": len(raw),
            "mb": round(len(raw) / 1024 / 1024, 2),
            "sha256": hashlib.sha256(raw).hexdigest()[:16],
            "mtime": time.strftime("%Y-%m-%d %H:%M", time.localtime(os.path.getmtime(p))),
            "deps": deps,
        })
    return out


def count_scale():
    def walk(base, exts):
        n = lines = 0
        for dp, _, fn in os.walk(os.path.join(ROOT, base)):
            for f in fn:
                if f.endswith(exts):
                    n += 1
                    with open(os.path.join(dp, f), encoding="utf-8", errors="ignore") as fh:
                        lines += sum(1 for _ in fh)
        return {"files": n, "lines": lines}

    tests = 0
    for dp, _, fn in os.walk(os.path.join(ROOT, "engine/src")):
        for f in fn:
            if f.endswith(".rs"):
                with open(os.path.join(dp, f), encoding="utf-8", errors="ignore") as fh:
                    tests += fh.read().count("#[test]")
    return {
        "engine_src_rs": walk("engine/src", (".rs",)),
        "engine_all_rs": walk("engine", (".rs",)),
        "win_src_cpp_h": walk("platform/windows/src", (".cpp", ".h")),
        "win_all_cpp_h": walk("platform/windows", (".cpp", ".h")),
        "rust_test_fns": tests,
    }


def collect_dicts():
    out = {}
    for name, rel in DICTS.items():
        p = os.path.join(ROOT, rel)
        if not os.path.exists(p):
            out[name] = {"missing": True}
            continue
        info = {"bytes": os.path.getsize(p), "mb": round(os.path.getsize(p) / 1048576, 2),
                "tables": {}}
        try:
            c = sqlite3.connect(p)
            for (t,) in c.execute("select name from sqlite_master where type='table'"):
                try:
                    info["tables"][t] = c.execute(f"select count(*) from [{t}]").fetchone()[0]
                except Exception as ex:
                    info["tables"][t] = f"ERR:{ex}"
            c.close()
        except Exception as ex:
            info["error"] = str(ex)
        out[name] = info
    for rel in ["resources/system_dict.db.bin", "resources/bigram.db.bin"]:
        p = os.path.join(ROOT, rel)
        out[os.path.basename(rel)] = {"bytes": os.path.getsize(p)} if os.path.exists(p) else {"missing": True}
    return out


def collect_v4_state():
    """V4 收口度量：默认开关、IPC 版本、容器行为、链接依赖。"""
    def grep(rel, pat, limit=8):
        p = os.path.join(ROOT, rel)
        if not os.path.exists(p):
            return []
        hits = []
        with open(p, encoding="utf-8", errors="ignore") as f:
            for i, line in enumerate(f, 1):
                if re.search(pat, line):
                    hits.append({"line": i, "text": line.strip()[:160]})
                    if len(hits) >= limit:
                        break
        return hits
    tsf_entry_rel = (
        "platform/windows/src/tsf_module.cpp"
        if os.path.exists(os.path.join(ROOT, "platform/windows/src/tsf_module.cpp"))
        else "platform/windows/src/tsf_text_service.cpp"
    )
    return {
        "use_server_render_default": grep("platform/windows/include/config_reader.h", r"use_server_render\s*="),
        "use_server_engine_default": grep("platform/windows/include/config_reader.h", r"use_server_engine\s*="),
        "ipc_version": grep("platform/windows/include/render_ipc.h",
                            r"constexpr uint32_t k\w*IpcVersion\s*=", limit=2),
        "container_force": grep(tsf_entry_rel, r"m_useServerEngine\s*=|IsAppContainerProcess\(\)"),
        "ime_link_engine": grep("platform/windows/CMakeLists.txt", r"target_link_libraries\(taishen_ime(_imm32)? PRIVATE taishen_engine"),
        "ime_link_d2d": grep("platform/windows/CMakeLists.txt", r"^\s*(d2d1|dwrite)\s*$"),
    }


def run_exam(tag):
    exe = os.path.join(ROOT, "engine/target/release/examples/exam.exe")
    if not os.path.exists(exe):
        return {"error": "exam.exe 未构建（先跑 cargo build --release --examples）"}
    r = run([exe], timeout=600)
    text = r["out"] + r["err"]
    grades = {}
    for m in re.finditer(r"^(L[1-5])\s+\S+\s+(\d+)/(\d+)\s+([\d.]+%)", text, re.M):
        grades[m.group(1)] = {"pass": int(m.group(2)), "total": int(m.group(3)), "pct": m.group(4)}
    tot = re.search(r"总计\s+(\d+)/(\d+)\s+通过（([\d.]+)%）", text)
    known = re.search(r"已知缺陷\s+(\d+)/(\d+)\s+通过（([\d.]+)%", text)
    return {
        "exit": r["rc"],
        "grades": grades,
        "total": {"pass": int(tot.group(1)), "all": int(tot.group(2)), "pct": tot.group(3)} if tot else None,
        "known_defect": {"pass": int(known.group(1)), "all": int(known.group(2)),
                         "pct": known.group(3)} if known else None,
        "candidate_lines_sha": hashlib.sha256(
            "\n".join(l.strip() for l in text.splitlines()
                      if l.strip().startswith(("[L", "实际", "总计"))).encode()).hexdigest()[:16],
    }


def run_lat_and_mem():
    exe = os.path.join(ROOT, "engine/target/release/examples/lat_probe.exe")
    if not os.path.exists(exe):
        return {"error": "lat_probe.exe 未构建"}
    env = dict(os.environ, TS_MEM_REPORT="1")
    peak_priv = peak_ws = 0
    p = subprocess.Popen([exe], cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                         text=True, encoding="utf-8", errors="ignore", env=env)
    if psutil:
        proc = psutil.Process(p.pid)
        while p.poll() is None:
            try:
                mi = proc.memory_info()
                peak_priv = max(peak_priv, mi.private)
                peak_ws = max(peak_ws, mi.rss)
            except Exception:
                break
            time.sleep(0.01)
    out = p.communicate()[0] or ""
    rows = []
    for line in out.splitlines():
        m = re.search(r"(\w+)\s+limit=(\d+)\s+total=\s*(\d+)us\s+peak=\s*(\d+)us", line)
        if m:
            rows.append({"input": m.group(1), "limit": int(m.group(2)),
                         "total_us": int(m.group(3)), "peak_us": int(m.group(4))})
    bigram = re.search(r"bigram \.bin 加载: (\d+) 前词 / (\d+) 对（(\d+)ms）", out)
    return {
        "latency_rows": rows,
        "peak_us_max": max((r["peak_us"] for r in rows), default=None),
        "peak_private_mb": round(peak_priv / 1048576, 2) if psutil else None,
        "peak_workset_mb": round(peak_ws / 1048576, 2) if psutil else None,
        "bigram_load": {"prev": int(bigram.group(1)), "pairs": int(bigram.group(2)),
                        "ms": int(bigram.group(3))} if bigram else None,
    }


def run_determinism():
    e1 = run_exam("r1")
    e2 = run_exam("r2")
    if "error" in e1 or "error" in e2:
        return {"error": "exam 不可用"}
    return {"run1": e1["candidate_lines_sha"], "run2": e2["candidate_lines_sha"],
            "identical": e1["candidate_lines_sha"] == e2["candidate_lines_sha"]}


def run_cargo_test():
    r = run(["cargo", "test", "--release", "--manifest-path", "engine/Cargo.toml",
             "--", "--test-threads=1"], timeout=1800)
    text = r["out"] + r["err"]
    summary = {}
    for m in re.finditer(r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored", text):
        key = m.group(1)
        s = summary.setdefault(key, {"passed": 0, "failed": 0, "ignored": 0})
        s["passed"] += int(m.group(2))
        s["failed"] += int(m.group(3))
        s["ignored"] += int(m.group(4))
    return {"exit": r["rc"], "summary": summary}


def to_md(d):
    L = []
    L.append(f"# 泰深输入法基线报告 v{d['version']['cargo']}")
    L.append("")
    L.append(f"- 采集时间：{d['collected_at']}")
    L.append(f"- commit：`{d['version']['git']}`（工作区{'有' if d['version'].get('dirty') else '无'}未提交改动）")
    L.append(f"- 词库版本：`{(d['version']['dict'] or {}).get('version')}`")
    L.append("")
    L.append("## 1. 构建产物与依赖（V4 打薄基准）")
    L.append("")
    L.append("| 产物 | 体积 | SHA256(16) | 依赖命中 |")
    L.append("|---|---:|---|---|")
    for b in d["binaries"]:
        if b.get("missing"):
            L.append(f"| `{b['path']}` | 缺失 | - | - |")
        else:
            L.append(f"| `{b['path']}` | {b['mb']} MB | `{b['sha256']}` | {', '.join(b['deps']) or '无'} |")
    L.append("")
    L.append("## 2. V4 收口状态")
    L.append("")
    v4 = d["v4"]
    for k, label in [("use_server_render_default", "普通宿主渲染默认"),
                     ("use_server_engine_default", "普通宿主引擎默认"),
                     ("ipc_version", "IPC 协议版本")]:
        vals = "; ".join(x["text"] for x in v4[k]) or "未找到"
        L.append(f"- {label}：{vals}")
    L.append(f"- ime.dll 仍链接引擎静态库：{'是' if v4['ime_link_engine'] else '否'}")
    L.append(f"- ime.dll 仍链接 d2d1/dwrite：{len(v4['ime_link_d2d'])} 处")
    L.append("")
    L.append("## 3. 代码规模")
    L.append("")
    sc = d["scale"]
    L.append("| 口径 | 文件数 | 行数 |")
    L.append("|---|---:|---:|")
    for k, label in [("engine_src_rs", "engine/src (*.rs)"),
                     ("engine_all_rs", "engine/** (*.rs，含 examples/tests)"),
                     ("win_src_cpp_h", "platform/windows/src (cpp+h)"),
                     ("win_all_cpp_h", "platform/windows/** (cpp+h，含 test)")]:
        L.append(f"| {label} | {sc[k]['files']} | {sc[k]['lines']} |")
    L.append(f"\nRust 单元测试函数：**{sc['rust_test_fns']}**")
    L.append("")
    L.append("## 4. 词库")
    L.append("")
    L.append("| 文件 | 体积 | 词条 |")
    L.append("|---|---:|---|")
    for name, info in d["dicts"].items():
        if info.get("missing"):
            L.append(f"| `{name}` | 缺失 | - |")
        elif "tables" in info:
            t = ", ".join(f"{k}={v}" for k, v in info["tables"].items() if k != "sqlite_sequence")
            L.append(f"| `{name}` | {info.get('mb')} MB | {t} |")
        else:
            L.append(f"| `{name}` | {info['bytes']} B | （预编译索引） |")
    L.append("")
    L.append("## 5. 考试成绩")
    L.append("")
    ex = d["exam"]
    if "error" in ex:
        L.append(f"未采集：{ex['error']}")
    else:
        L.append("| 分级 | 通过 | 占比 |")
        L.append("|---|---:|---:|")
        for g in sorted(ex["grades"]):
            x = ex["grades"][g]
            L.append(f"| {g} | {x['pass']}/{x['total']} | {x['pct']} |")
        if ex["total"]:
            t = ex["total"]
            L.append(f"| **总计** | **{t['pass']}/{t['all']}** | **{t['pct']}%** |")
        if ex["known_defect"]:
            k = ex["known_defect"]
            L.append(f"\n已知缺陷组：{k['pass']}/{k['all']}（{k['pct']}%）")
        L.append(f"\nexam 退出码：{ex['exit']}（0=全通过，1=有 FAIL）")
    L.append("")
    L.append("## 6. 性能与内存")
    L.append("")
    lat = d["latency"]
    if "error" in lat:
        L.append(f"未采集：{lat['error']}")
    else:
        L.append("| 输入 | limit | 总耗时(us) | 单键峰值(us) |")
        L.append("|---|---:|---:|---:|")
        for r in lat["latency_rows"]:
            L.append(f"| `{r['input']}` | {r['limit']} | {r['total_us']} | {r['peak_us']} |")
        if lat.get("peak_private_mb"):
            L.append(f"\n峰值私有内存：**{lat['peak_private_mb']} MB**，峰值工作集 {lat['peak_workset_mb']} MB")
        if lat.get("bigram_load"):
            b = lat["bigram_load"]
            L.append(f"\nbigram 加载：{b['prev']} 前词 / {b['pairs']} 对，**{b['ms']} ms**")
    L.append("")
    L.append("## 7. 确定性")
    L.append("")
    det = d["determinism"]
    if "error" in det:
        L.append(f"未采集：{det['error']}")
    else:
        L.append(f"两次独立进程候选输出哈希：`{det['run1']}` / `{det['run2']}` → "
                 f"**{'一致' if det['identical'] else '不一致（存在非确定性）'}**")
    L.append("")
    if d.get("cargo_test"):
        ct = d["cargo_test"]
        L.append("## 8. 单元测试")
        L.append("")
        for k, v in ct["summary"].items():
            L.append(f"- {k}: passed {v['passed']} / failed {v['failed']} / ignored {v['ignored']}")
        L.append(f"\n退出码：{ct['exit']}")
        L.append("")
    L.append("## 9. 未覆盖项（本基线不采集，需人工/另跑）")
    L.append("")
    L.append("- CTest 平台层测试（需先构建 `platform/windows/build` 的 `test_*` 目标）")
    L.append("- 端到端 TSF→IPC→引擎→D2D 延迟（需活体宿主 + QPC 埋点）")
    L.append("- 真实用户词库回归（`exam --user-db`，仅人工入口）")
    L.append("- 多宿主兼容矩阵（Edge/VS Code/Office/微信/IMM32 游戏）")
    L.append("")
    L.append("## 10. 复测方法")
    L.append("")
    L.append("```")
    L.append("cargo build --release --examples --manifest-path engine/Cargo.toml")
    L.append("python tools/baseline/collect_baseline.py --out docs/baseline/v<新版本>")
    L.append("```")
    L.append("")
    L.append("对比方式：`python tools/baseline/collect_baseline.py --diff <旧 baseline.json>`")
    return "\n".join(L)


def resolve_input_path(path_arg):
    if os.path.exists(path_arg):
        return path_arg
    rel = os.path.join(ROOT, path_arg)
    return rel if os.path.exists(rel) else path_arg


def diff_baselines(old, cur, old_label, cur_label):
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    print(f"diff 模式：对比 {old_label} -> {cur_label}")
    failed = 0

    # 1. binaries[].deps + volume delta
    old_bins = {b["path"]: b for b in (old.get("binaries") or []) if "path" in b}
    cur_bins = {b["path"]: b for b in (cur.get("binaries") or []) if "path" in b}
    old_all_deps = set().union(*(set(b.get("deps") or []) for b in old_bins.values()))
    dep_regressions = []
    for path, cur_b in cur_bins.items():
        if cur_b.get("missing"):
            print(f"  [SKIP] {path}: 当前基线未构建（missing）")
            continue
        old_b = old_bins.get(path, {})
        old_deps = set(old_b.get("deps") or [])
        cur_deps = set(cur_b.get("deps") or [])
        added = sorted(cur_deps - old_deps)
        old_mb = old_b.get("mb")
        cur_mb = cur_b.get("mb")
        mb_delta = f" ({cur_mb - old_mb:+.2f} MB)" if old_mb is not None and cur_mb is not None else ""
        is_server_target = os.path.basename(path).lower() == "taishen_server.exe"
        if added and is_server_target:
            truly_new = sorted(set(added) - old_all_deps)
            migrated = sorted(set(added) & old_all_deps)
            if truly_new:
                dep_regressions.append(f"{path} 新增依赖: {', '.join(truly_new)}")
                print(f"  [FAIL] {path}: {cur_mb} MB{mb_delta} | 新增依赖: {', '.join(truly_new)}")
            else:
                print(
                    f"  [INFO] {path}: {cur_mb} MB{mb_delta} | 由 ime/engine 迁移依赖: {', '.join(migrated)}（无全局新增依赖）"
                )
        elif added:
            dep_regressions.append(f"{path} 新增依赖: {', '.join(added)}")
            print(f"  [FAIL] {path}: {cur_mb} MB{mb_delta} | 新增依赖: {', '.join(added)}")
        else:
            print(f"  [PASS] {path}: {cur_mb} MB{mb_delta} | 无新增依赖")
    if dep_regressions:
        failed += 1

    # 2. exam.total.pass (>= max(old_pass, 279)) + L1..L5 detail
    old_exam = old.get("exam") or {}
    cur_exam = cur.get("exam") or {}
    old_total = old_exam.get("total") or {}
    cur_total = cur_exam.get("total") or {}
    if "error" in cur_exam or "pass" not in cur_total:
        print(f"  [FAIL] exam: 缺失或采集失败 ({cur_exam.get('error', 'no total')})")
        failed += 1
    else:
        old_pass = int(old_total.get("pass", 0))
        cur_pass = int(cur_total["pass"])
        cur_all = int(cur_total.get("all", 0))
        floor = max(old_pass, 279)
        if cur_pass < floor:
            print(f"  [FAIL] exam: {cur_pass}/{cur_all} < 门禁 {floor} (old={old_pass})")
            failed += 1
        else:
            print(f"  [PASS] exam: {cur_pass}/{cur_all} (门禁 >= {floor}, old={old_pass})")
        old_grades = old_exam.get("grades") or {}
        cur_grades = cur_exam.get("grades") or {}
        for g in sorted(set(old_grades) | set(cur_grades)):
            og = old_grades.get(g) or {}
            cg = cur_grades.get(g) or {}
            op, cp = og.get("pass"), cg.get("pass")
            if op is not None and cp is not None and cp < op:
                print(f"  [WARN] exam {g}: {cp}/{cg.get('total')} < old {op}/{og.get('total')}")

    # 3. determinism.identical
    cur_det = cur.get("determinism") or {}
    if "error" in cur_det or cur_det.get("identical") is not True:
        print(f"  [FAIL] determinism: identical={cur_det.get('identical')} "
              f"({cur_det.get('run1')} / {cur_det.get('run2')}, err={cur_det.get('error')})")
        failed += 1
    else:
        print(f"  [PASS] determinism: identical=True ({cur_det.get('run1')})")

    # 4. latency.peak_private_mb (<= 42.0 and <= old + 3.0)
    old_mem = (old.get("latency") or {}).get("peak_private_mb")
    cur_lat = cur.get("latency") or {}
    cur_mem = cur_lat.get("peak_private_mb")
    if "error" in cur_lat:
        print(f"  [FAIL] latency: 采集失败 ({cur_lat.get('error')})")
        failed += 1
    elif cur_mem is None:
        print("  [SKIP] peak_private_mb: 缺失（未安装 psutil）")
    else:
        limit = 42.0 if old_mem is None else min(42.0, float(old_mem) + 3.0)
        if float(cur_mem) > 42.0 or (old_mem is not None and float(cur_mem) > float(old_mem) + 3.0):
            print(f"  [FAIL] peak_private_mb: {cur_mem} MB > 门禁 {limit:.2f} MB (old={old_mem})")
            failed += 1
        else:
            delta = f" ({float(cur_mem) - float(old_mem):+.2f} MB)" if old_mem is not None else ""
            print(f"  [PASS] peak_private_mb: {cur_mem} MB{delta} (门禁 <= 42.0 且 <= old+3.0)")

    if failed:
        print(f"[FAIL] 基线对比共 {failed} 项硬指标劣化")
        return 1
    print("[PASS] 基线对比四项硬指标全部通过")
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="docs/baseline/latest")
    ap.add_argument("--with-tests", action="store_true")
    ap.add_argument("--diff", help="与旧 baseline.json 对比")
    ap.add_argument("--cur", help="diff 模式下的当前 baseline.json（默认 <out>/baseline.json）")
    args = ap.parse_args()

    if args.diff:
        old_path = resolve_input_path(args.diff)
        cur_path = resolve_input_path(args.cur) if args.cur else os.path.join(ROOT, args.out, "baseline.json")
        if not os.path.exists(old_path):
            print(f"[FAIL] 旧基线不存在: {old_path}")
            sys.exit(1)
        if not os.path.exists(cur_path):
            print(f"[FAIL] 当前基线不存在: {cur_path}")
            sys.exit(1)
        with open(old_path, encoding="utf-8") as f:
            old = json.load(f)
        with open(cur_path, encoding="utf-8") as f:
            cur = json.load(f)
        sys.exit(diff_baselines(old, cur, old_path, cur_path))

    data = {
        "collected_at": time.strftime("%Y-%m-%d %H:%M:%S"),
        "version": read_version(),
        "binaries": collect_binaries(),
        "v4": collect_v4_state(),
        "scale": count_scale(),
        "dicts": collect_dicts(),
        "exam": run_exam("main"),
        "latency": run_lat_and_mem(),
        "determinism": run_determinism(),
    }
    if args.with_tests:
        data["cargo_test"] = run_cargo_test()

    outdir = os.path.join(ROOT, args.out)
    os.makedirs(outdir, exist_ok=True)
    with open(os.path.join(outdir, "baseline.json"), "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False, indent=2)
    with open(os.path.join(outdir, "baseline.md"), "w", encoding="utf-8") as f:
        f.write("\ufeff" + to_md(data))
    print("已生成：")
    print(" ", os.path.join(args.out, "baseline.json"))
    print(" ", os.path.join(args.out, "baseline.md"))
    print("exam 总计：", data["exam"].get("total"))
    print("峰值内存：", data["latency"].get("peak_private_mb"), "MB")
    print("确定性：", data["determinism"].get("identical"))


if __name__ == "__main__":
    main()
