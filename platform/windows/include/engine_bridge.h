#ifndef TAISHEN_ENGINE_BRIDGE_H
#define TAISHEN_ENGINE_BRIDGE_H

#ifdef __cplusplus
extern "C" {
#endif

/// 初始化引擎，dict_path 为系统词库路径（NULL 则回退内置词库），返回 0 成功
int engine_init(const char* dict_path);

/// 设置用户词库路径（V0.2.2），user_path 为 NULL/空则禁用，返回 0 成功
int engine_set_user_dict_path(const char* user_path);

/// 处理按键，返回候选词数量。-1 表示未初始化
int engine_process_key(int ch);

/// 退格，返回当前候选词数量
int engine_backspace(void);

/// 获取当前拼音串，返回字符串长度（含 null 终止符）
int engine_get_pinyin_str(char* buf, int buf_len);

/// 获取候选词总数
int engine_get_candidate_count(void);

/// 获取指定候选词，返回字符串长度（含 null 终止符）
int engine_get_candidate(int index, char* buf, int buf_len);

/// 选择候选词，提交文本写入 buf，返回文本长度
int engine_select_candidate(int index, char* buf, int buf_len);

/// 以词定字（V0.2.24）：取当前页首个候选首/末字符上屏（first: 1=首 0=末），返回文本长度
int engine_take_char(int first, char* buf, int buf_len);

/// 加载拆字反查词库（V0.2.25），NULL = 仅内置空表
int engine_set_radical_path(const char* path);

/// 设置候选词数量上限，返回 0 成功 / -1 未初始化
int engine_set_candidate_count(int count);

/// 设置模糊音开关（RIME 拼写变体，0.1.14），返回 0 成功 / -1 未初始化
int engine_set_fuzzy(int enabled);

/// 查询模糊音开关：1=开 / 0=关 / -1 未初始化
int engine_get_fuzzy(void);

/// 设置双拼模式（RIME 微软双拼方案，0.1.14），返回 0 成功 / -1 未初始化
int engine_set_shuangpin(int enabled);

/// 查询双拼模式：1=开 / 0=关 / -1 未初始化
int engine_get_shuangpin(void);

/// 设置智能纠错开关（键盘相邻键容错，0.2.10），返回 0 成功 / -1 未初始化
int engine_set_correction(int enabled);

/// 查询智能纠错开关：1=开 / 0=关 / -1 未初始化
int engine_get_correction(void);

/// 设置中英混输开关（中文模式候选末尾英文候选，0.2.8），返回 0 成功 / -1 未初始化
int engine_set_mix_mode(int enabled);

/// 查询中英混输开关：1=开 / 0=关 / -1 未初始化
int engine_get_mix_mode(void);

/// 设置简繁转换开关（候选输出转繁体，0.2.11），返回 0 成功 / -1 未初始化
int engine_set_traditional(int enabled);

/// 查询简繁转换开关：1=开 / 0=关 / -1 未初始化
int engine_get_traditional(void);

/// 设置快捷短语开关（简码→短语，0.2.12），返回 0 成功 / -1 未初始化
int engine_set_phrase_enabled(int enabled);

/// 查询快捷短语开关：1=开 / 0=关 / -1 未初始化
int engine_get_phrase_enabled(void);

/// 加载外部短语文件（0.2.12，每行 code=text，# 注释），NULL = 仅内置
int engine_set_phrase_path(const char* path);

/// 设置英文模式，返回 0 成功 / -1 未初始化
int engine_set_ascii_mode(int enabled);

/// 查询英文模式：1=英文 / 0=中文 / -1 未初始化
int engine_get_ascii_mode(void);

/// 设置中英标点开关（P0-2）：1=英文标点透传 / 0=中文标点全角化
int engine_set_ascii_punct(int enabled);

/// 查询中英标点开关：1=英文标点 / 0=中文标点 / -1 未初始化
int engine_get_ascii_punct(void);

/// 清空引擎状态
void engine_reset(void);

/// 翻页。delta: +1 下一页 / -1 上一页。返回当前页候选数
int engine_page(int delta);

/// 获取当前页码（0 起）
int engine_get_current_page(void);

/// 获取总页数
int engine_get_total_pages(void);

/// 销毁引擎
void engine_destroy(void);

#ifdef __cplusplus
}
#endif

#endif // TAISHEN_ENGINE_BRIDGE_H
