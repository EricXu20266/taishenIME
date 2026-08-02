#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
泰深输入法 — 系统词库构建工具
生成 system_dict.db（SQLite: pinyin, word, frequency）

词库组成：
  1. 单字：GB2312 一级汉字 3755 个 + pypinyin 注音（频率按 GB2312 区号递减——区越小越常用）
  2. 多字词：内置常用词表（日常 + IT + AI 领域） + pypinyin 注音
  3. 简拼列：每个词条的声母串（如 中国→zg），供引擎简拼索引

用法：python tools/build_dict.py
输出：resources/system_dict.db（覆盖）
"""
import os
import sqlite3
import sys

try:
    from pypinyin import pinyin, Style
except ImportError:
    print("需要 pypinyin: pip install pypinyin")
    sys.exit(1)

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT_DB = os.path.join(ROOT, "resources", "system_dict.db")

# ---------------------------------------------------------------------------
# 1. 单字词库：GB2312 一级汉字 3755 个
# ---------------------------------------------------------------------------
def gb2312_level1_chars():
    """GB2312 一级汉字：区 16-55，每区 94 位。共 3755 字。"""
    chars = []
    for qu in range(16, 56):
        for wei in range(1, 95):
            try:
                ch = bytes([qu + 0xA0, wei + 0xA0]).decode("gb2312")
                chars.append(ch)
            except Exception:
                pass
    return chars


def char_frequency(index):
    """单字频率：区号越小越常用。区16(最常用)≈4000，区55≈100。"""
    qu = index // 94
    return max(100, 4000 - (qu - 16) * 80)


# ---------------------------------------------------------------------------
# 2. 常用多字词表（日常 + IT/AI 领域，频率按常用度递减）
# ---------------------------------------------------------------------------
COMMON_WORDS = [
    # 高频双字词（日常）
    "我们", "你们", "他们", "她们", "自己", "一个", "这个", "那个", "什么", "怎么",
    "因为", "所以", "但是", "可是", "然后", "如果", "虽然", "而且", "并且", "或者",
    "没有", "现在", "已经", "正在", "曾经", "将来", "过去", "最近", "今天", "明天",
    "昨天", "上午", "下午", "晚上", "时候", "时间", "现在", "中国", "北京", "上海",
    "广州", "深圳", "杭州", "南京", "成都", "武汉", "西安", "重庆", "天津", "苏州",
    "工作", "学习", "生活", "朋友", "家人", "孩子", "老师", "学生", "医生", "律师",
    "公司", "企业", "单位", "学校", "医院", "银行", "市场", "经济", "政治", "文化",
    "历史", "科学", "技术", "艺术", "音乐", "电影", "游戏", "体育", "新闻", "天气",
    "健康", "医疗", "保险", "金融", "股票", "基金", "房产", "汽车", "能源", "环保",
    "教育", "社会", "法律", "军事", "外交", "科技", "航天", "航空", "铁路", "公路",
    "手机", "电脑", "网络", "微信", "微博", "抖音", "快手", "视频", "直播", "电商",
    "支付", "外卖", "打车", "旅游", "酒店", "电影", "音乐", "游戏", "通讯", "办公",
    "大家", "人们", "问题", "方法", "结果", "过程", "研究", "开发", "设计", "测试",
    "发布", "更新", "维护", "部署", "运行", "性能", "优化", "架构", "接口", "模块",
    "组件", "配置", "日志", "监控", "统计", "分析", "交互", "体验", "社区", "开放",
    "合作", "创新", "质量", "安全", "服务", "用户", "客户", "数据", "信息", "知识",
    # 互联网/AI/IT 领域
    "互联网", "人工智能", "机器学习", "深度学习", "大数据", "云计算", "区块链",
    "物联网", "虚拟现实", "增强现实", "数字孪生", "芯片", "半导体", "新能源",
    "光伏", "风电", "储能", "电池", "电动车", "自动驾驶", "智能驾驶", "机器人",
    "无人机", "量子计算", "脑机接口", "生物科技", "基因编辑", "纳米", "计算机",
    "程序员", "工程师", "产品经理", "项目经理", "创业者", "投资者", "分析师",
    "大模型", "多模态", "生成式", "自然语言", "计算机视觉", "语音识别", "图像识别",
    "推荐系统", "搜索引擎", "操作系统", "数据库", "服务器", "客户端", "前端", "后端",
    "全栈", "算法", "模型", "训练", "推理", "微调", "提示词", "智能体", "自动化",
    # 短视频/自媒体（Eric 领域）
    "短视频", "直播", "直播间", "内容创作", "自媒体", "粉丝", "流量", "爆款",
    "涨粉", "变现", "广告", "带货", "运营", "选题", "脚本", "剪辑", "拍摄",
    "口播", "封面", "标题", "话题", "热点", "算法推荐", "涨粉", "人设",
    # 常用短语
    "你好", "您好", "谢谢", "再见", "对不起", "没关系", "请问", "再见", "晚安",
    "早上好", "下午好", "晚上好", "周末快乐", "生日快乐", "新年快乐", "恭喜发财",
    "好的", "可以", "不行", "没问题", "知道了", "明白了", "收到", "同意", "反对",
    # 姓名常用
    "小明", "小红", "小王", "小李", "张伟", "王芳", "李娜", "刘洋", "陈静", "杨帆",
    "赵磊", "黄勇", "周杰", "吴敏", "徐强", "孙丽", "马超", "朱婷", "胡军", "郭涛",
]

# 补充已有 raw_dict 中的词（避免丢失人工整理词条）
RAW_EXTRA = [
    # 输入法相关
    "拼音", "双拼", "全拼", "候选", "上屏", "词库", "词频", "模糊音", "键盘",
    # IT 常用
    "加密", "解密", "速度", "代码", "编程", "脚本", "编译", "调试", "部署",
    "软件", "硬件", "系统", "网络", "数据库", "服务器", "客户端",
    # 其他
    "再见", "帮助", "联系", "欢迎",
]

# 合并去重（保持顺序）
def merge_words():
    seen = set()
    words = []
    for w in COMMON_WORDS + RAW_EXTRA:
        if w not in seen:
            seen.add(w)
            words.append(w)
    return words


# ---------------------------------------------------------------------------
# 构建
# ---------------------------------------------------------------------------
def build():
    entries = []  # (pinyin, word, frequency)

    # 单字（频率按 GB2312 区号）
    chars = gb2312_level1_chars()
    for idx, ch in enumerate(chars):
        py = pinyin(ch, style=Style.NORMAL, heteronym=False)[0][0]
        if not py:
            continue
        entries.append((py, ch, char_frequency(idx)))

    # 多字词（频率 5000 起递减，高于单字——多字词更精准优先）
    words = merge_words()
    base_freq = 5000
    for i, w in enumerate(words):
        py_list = pinyin(w, style=Style.NORMAL, heteronym=False)
        py = "".join(p[0] for p in py_list if p[0])
        # 校验：音节数必须等于字数（排除注音失败/含数字字母的词）
        if len(py_list) != len(w):
            continue
        freq = max(500, base_freq - i * 3)
        entries.append((py, w, freq))

    # 写 SQLite
    if os.path.exists(OUT_DB):
        os.remove(OUT_DB)
    conn = sqlite3.connect(OUT_DB)
    conn.execute("CREATE TABLE system_dict (pinyin TEXT, word TEXT, frequency INTEGER)")
    conn.executemany(
        "INSERT INTO system_dict (pinyin, word, frequency) VALUES (?, ?, ?)",
        entries,
    )
    conn.execute(
        "CREATE INDEX idx_pinyin ON system_dict(pinyin, frequency DESC)"
    )
    conn.commit()
    conn.close()

    print(f"词库构建完成: {len(entries)} 条 → {OUT_DB}")
    print(f"  单字: {len(chars)} 多字词: {len(words)}")
    # 抽查
    conn = sqlite3.connect(OUT_DB)
    for py in ["zhong", "zhongguo", "ni", "ai", "nihaoma"]:
        rows = conn.execute(
            "SELECT word FROM system_dict WHERE pinyin=? ORDER BY frequency DESC LIMIT 5",
            (py,),
        ).fetchall()
        print(f"  {py}: {[r[0] for r in rows]}")
    conn.close()


if __name__ == "__main__":
    build()
