# SPEC: 词库扩容（Root #1，V0.2.16）

> 对应 ARCHITECT.md Root #1「词库」
> 关联 DEV-TRACKER: 0.2.16 词库扩容：吸收 rime-ice ext + tencent 词库

---

## 一、需求

当前 system_dict.db 5.8 万条（8105 常用字全量 + base top 5 万）。对标 rime-ice，补充两块词库：

| 词库 | 文件 | 规模 | 特点 |
|------|------|------|------|
| ext（扩展词库） | ext.dict.yaml | 33.9 万 | 自带拼音注音（含多音字），3 列标准格式 |
| tencent（腾讯词向量） | tencent.dict.yaml | 98 万 | **无拼音**，需 pypinyin 自动注音；3 字词 22 万（人名/地名/机构） |

**目标**：5.8 万 → **62 万+**（ext 全量 33.9 万 + tencent 3 字词 22 万 + 现有 5.8 万）

**用户价值**：人名/地名/专业术语/长词覆盖率大幅提升，减少"打不出来"。

## 二、策略与约束

### 吸收策略

| 词库 | 策略 | 理由 |
|------|------|------|
| 8105 | 全量保留（现逻辑） | 单字覆盖 |
| base | top 5 万保留（现逻辑） | 高频词控制体积 |
| ext | **全量吸收** | 自带准确注音，扩展词全覆盖 |
| tencent | **截取 3 字词**（~22 万） | 实测 3-5 字 85 万全收内存过重；3 字词是长词主力（人名/地名/机构），4 字+ 后续补充 |

### 内存约束

引擎全量加载到内存（每个词条为每个前缀建索引）：
- 现有 5.8 万条 ≈ 40MB 索引
- 62 万条 ≈ 250MB 索引（输入法常驻进程，可接受）
- 若 tencent 全量 98 万吸收 → 125 万+ 词条，索引膨胀 800MB+，不可接受

### 多音字注音

- ext 词库自带注音（rime-ice 已修正多音字）→ 全量信任
- tencent 无注音 → pypinyin 默认读音（heteronym=False 取第一音）；
  多音字错误率 ~5%，rime-ice 官方也是靠字表权重兜底，此处简化为默认音

## 三、数据模型

不变——system_dict.db 表结构（pinyin, word, frequency）保持。

## 四、接口

无 FFI/引擎改动。仅 `tools/build_dict.py` 扩展：

```
download()：新增 ext.dict.yaml + tencent.dict.yaml（走代理）
parse_tencent()：新解析器（2 列：word + freq，无拼音）
  过滤：len(word) 3-5 字（用字符数判断，跳过含数字/字母/标点的词）
  pypinyin 自动注音 → (word, pinyin_nospace, freq=100)
merge()：ext 全量 + tencent 截取并入现有流程
```

## 五、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | build_dict.py 扩展（下载 + 解析 + 注音 + 合并） | tools/build_dict.py | python 运行成功 |
| 2 | 重建 system_dict.db，验证词条数与抽样查询 | resources/system_dict.db | 抽样验证 |
| 3 | cargo test（引擎加载大词库） | engine | 测试通过 |
| 4 | 全链路验证 + commit | — | build + test + biome |

## 六、测试用例

- 词条数 50 万+（5.8 万 → 57 万左右）
- 抽样查询：rengongzhineng（人工智能）、diannao（电脑）等常用词仍在
- 新增词命中：ext 中的专有名词（如阿巴拉契亚 a ba la qi ya）、tencent 三字词
- 加载时间可接受（< 5s）
