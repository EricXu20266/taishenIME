#ifndef TAISHEN_ENGINE_BRIDGE_H
#define TAISHEN_ENGINE_BRIDGE_H

#ifdef __cplusplus
extern "C" {
#endif

/// 初始化引擎，dict_path 为系统词库路径（NULL 则回退内置词库），返回 0 成功
int engine_init(const char* dict_path);

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

/// 设置候选词数量上限，返回 0 成功 / -1 未初始化
int engine_set_candidate_count(int count);

/// 清空引擎状态
void engine_reset(void);

/// 销毁引擎
void engine_destroy(void);

#ifdef __cplusplus
}
#endif

#endif // TAISHEN_ENGINE_BRIDGE_H
