/// 资源 ID — 设置对话框
///
/// 对应 SPEC: docs/modules/settings-ui/SPEC.md
/// 定义 IDD_SETTINGS（主对话框）与 IDD_THEME_COLORS（配色子对话框）的控件 ID。

#pragma once

// ── 对话框 ──
#define IDD_SETTINGS         100
#define IDD_THEME_COLORS     101

// ── 主对话框：Tab 控件 ──
#define IDC_TAB_MAIN         1001

// ── 页 0 基础 ──
#define IDC_EDIT_CANDIDATE      1101
#define IDC_EDIT_FONT_FACE      1102
#define IDC_EDIT_FONT_SIZE      1103
#define IDC_CHK_INLINE_PREEDIT  1104
#define IDC_EDIT_LABEL_FORMAT   1105

// ── 页 1 输入 ──
#define IDC_CHK_FUZZY           1201
#define IDC_CHK_CORRECTION      1202
#define IDC_CHK_MIX_MODE        1203
#define IDC_CHK_TRADITIONAL     1204
#define IDC_CHK_SHUANGPIN       1205
#define IDC_COMBO_SCHEME        1206
#define IDC_CHK_PHRASE          1207
#define IDC_CHK_ASCII_PUNCT     1208
#define IDC_CHK_EMOJI           1209

// ── 页 2 外观 ──
#define IDC_COMBO_THEME         1301
#define IDC_BTN_THEME_COLORS    1302
#define IDC_EDIT_CORNER         1303
#define IDC_EDIT_HILITE_CORNER  1304
#define IDC_EDIT_PADDING        1305
#define IDC_EDIT_SPACING        1306

// ── 页 3 高级 ──
#define IDC_EDIT_APP_ASCII      1401
#define IDC_EDIT_DICT_PATH      1402
#define IDC_EDIT_USER_DICT_PATH 1403
#define IDC_EDIT_PHRASE_PATH    1404

// ── 底部按钮 ──
#define IDC_BTN_DEFAULTS        1501
#define IDC_BTN_OPEN_CONFIG     1502

// ── 配色子对话框：10 行，每行 选择按钮 ID = IDC_CLR_BTN_BASE + i，
//    HEX 文本 ID = IDC_CLR_HEX_BASE + i（i = 0..9）──
#define IDC_CLR_BTN_BASE        2000
#define IDC_CLR_HEX_BASE        2200
