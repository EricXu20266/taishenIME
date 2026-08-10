# tools/archive — 历史遗留词库构建脚本（归档）

> 词库构建职责已全部移交 **taishen-dict** 项目（https://github.com/EricXu20266/taishen-dict）。
> 本目录脚本仅供历史追溯，**不要运行**——产出的词库与现行 dict 管线不兼容。

## 归档清单

| 脚本 | 原用途 | 替代 |
|------|--------|------|
| build_system_dict.py | raw_dict.txt → system_dict.db（单表旧格式） | dict 管线 sqlite.py（双表） |
| build_clean_dict.py | V0.3.7 合规换源构建系统词库（dict 管线前身） | dict 管线 |
| build_clean_domains.py | V0.3.7 合规换源构建领域词库（dict 管线前身） | dict 管线 |
| build_common_db.py | common_dict.txt → common.db | dict 管线 build_common_db.py |
| build_domains_db.py | domains/*.txt → domains.db | dict 管线 domains_db.py |
| rebuild_dict_freq.py | 单字词频重建（早期工具） | dict 管线 curate/ 调校 |
| gen_modern_dict.py | 现代词表 LLM 补全生成 | dict 管线 modern.txt（curate/domains/） |
| build_domain_dict.py | 搜狗细胞词库 → 领域 txt（来源敏感） | 已弃用（合规换源） |

## 资源归档

- `resources/archive/raw_dict.txt` — 早期手工高频词表（pinyin\tword\tfreq），
  202 词已全部被 dict 产物覆盖（其中 3 词 2026-08-10 补入 common_dict.txt），
  仅供追溯保留。

## 现行工作流

```
taishen-dict: 改 curate/ 源 → python pipeline.py → python tools/sync_to_ime.py
taishenIME:   只消费 resources/ 下同步来的词库（system_dict.db / domains.db / common.db + VERSION.json）
```

词库版本见 `resources/VERSION.json`（同步时写入）。
