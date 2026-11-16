/// 中英文切换冒烟测试 — 独立 exe
///
/// 验证：
///   1. Ctrl+Space 切换模式
///   2. 英文模式下字母直通 committed
///   3. 中文模式下字母累积拼音
/// 返回 0 = 通过。

#include <windows.h>
#include <cstdio>
#include <string>

#include "tsf_keyevent.h"
#include "engine_bridge.h"

int wmain()
{
    engine_init(nullptr);
    engine_set_candidate_count(9);

    // 初始中文模式
    if (engine_get_ascii_mode() != 0) {
        printf("STEP1 FAIL: initial ascii_mode=%d\n", engine_get_ascii_mode());
        return 1;
    }
    printf("STEP1 OK initial chinese mode\n");

    // 中文模式：字母累积拼音
    {
        taishen::KeyEventResult r;
        const bool eat = taishen::HandleKeyDown('N', 0, r);
        if (!eat || !r.state_changed) {
            printf("STEP2 FAIL: chinese letter not eaten, eat=%d changed=%d\n", eat, r.state_changed);
            return 1;
        }
        char buf[64] = {0};
        engine_get_pinyin_str(buf, sizeof(buf));
        printf("STEP2 pinyin=%s\n", buf);
        if (strcmp(buf, "n") != 0) {
            printf("STEP2 FAIL: pinyin accumulate wrong: %s\n", buf);
            return 1;
        }
    }

    // Ctrl+Space 切换（模拟 Ctrl 按下）
    {
        keybd_event(VK_CONTROL, 0, 0, 0); // Ctrl 按下
        taishen::KeyEventResult r;
        const bool eat = taishen::HandleKeyDown(VK_SPACE, 0, r);
        keybd_event(VK_CONTROL, 0, KEYEVENTF_KEYUP, 0); // Ctrl 松开
        printf("STEP3 ctrl+space: eat=%d ascii=%d\n", eat, engine_get_ascii_mode());
        if (!eat || engine_get_ascii_mode() != 1) {
            printf("STEP3 FAIL: ctrl+space should switch to english\n");
            return 1;
        }
        printf("STEP3 OK switched to english\n");
    }

    // 英文模式：字母直通（0.1.15 契约：透传给应用，不吞键、不 committed）
    // 修复历史：旧版走 committed 提交但无组合时 CommitComposition 不写文本 → 字母丢失。
    // 透传是输入法英文模式的业界标准——应用直接接收按键上屏。
    {
        taishen::KeyEventResult r;
        const bool eat = taishen::HandleKeyDown('A', 0, r);
        printf("STEP4 english letter: eat=%d committed=%ls changed=%d\n", eat, r.committed.c_str(), r.state_changed);
        if (eat || !r.committed.empty() || r.state_changed) {
            printf("STEP4 FAIL: english letter should pass through (not eaten, no commit)\n");
            return 1;
        }
        // 拼音不累积
        char buf[64] = {0};
        engine_get_pinyin_str(buf, sizeof(buf));
        if (strcmp(buf, "") != 0) {
            printf("STEP4 FAIL: english mode should not accumulate pinyin: %s\n", buf);
            return 1;
        }
        printf("STEP4 OK passed through\n");
    }

    // 再 Ctrl+Space 切回中文
    {
        keybd_event(VK_CONTROL, 0, 0, 0);
        taishen::KeyEventResult r;
        taishen::HandleKeyDown(VK_SPACE, 0, r);
        keybd_event(VK_CONTROL, 0, KEYEVENTF_KEYUP, 0);
        printf("STEP5 switch back: ascii=%d\n", engine_get_ascii_mode());
        if (engine_get_ascii_mode() != 0) {
            printf("STEP5 FAIL: should switch back to chinese\n");
            return 1;
        }
        printf("STEP5 OK back to chinese\n");
    }

    // 多行展开/收起（0.2.14）：↓ 展开请求 / ↑ 收起请求
    {
        engine_process_key('z');
        engine_process_key('h');  // 有候选
        taishen::KeyEventResult r;
        const bool eatDown = taishen::HandleKeyDown(VK_DOWN, 0, r);
        printf("STEP6 down: eat=%d multirow=%d\n", eatDown, r.multirow_requested);
        if (!eatDown || !r.multirow_requested) {
            printf("STEP6 FAIL: down should request multirow expand\n");
            return 1;
        }
        taishen::KeyEventResult r2;
        const bool eatUp = taishen::HandleKeyDown(VK_UP, 0, r2);
        printf("STEP6 up: eat=%d multirow=%d collapse=%d\n", eatUp, r2.multirow_requested, r2.multirow_collapse);
        if (!eatUp || r2.multirow_requested || !r2.multirow_collapse) {
            printf("STEP6 FAIL: up should request multirow collapse\n");
            return 1;
        }
        // V0.3.x：新字母 → 自动复位多行（输入新词不再维持多列）
        taishen::KeyEventResult r3;
        taishen::HandleKeyDown('A', 0, r3);
        printf("STEP6 letter: collapse=%d\n", r3.multirow_collapse);
        if (!r3.multirow_collapse) {
            printf("STEP6 FAIL: new letter should reset multirow\n");
            return 1;
        }
        engine_reset();
        printf("STEP6 OK multirow toggle + auto-collapse\n");
    }

    // V0.2.36 vim_mode：vim 键透传（Esc / Ctrl+C / Ctrl+[），vimPassthrough 时强制不吞
    {
        if (!taishen::IsVimModeKey(VK_ESCAPE)) {
            printf("STEP7 FAIL: Esc should be vim key\n");
            return 1;
        }
        // 无 vim 模式：Esc 无候选时本来就透传（应用取消/退出插入）
        if (taishen::ShouldEatKey(VK_ESCAPE, false)) {
            printf("STEP7 FAIL: Esc should pass without vim mode\n");
            return 1;
        }
        // vim 模式：Esc 强制透传（vim 收到退出插入模式）
        if (taishen::ShouldEatKey(VK_ESCAPE, true)) {
            printf("STEP7 FAIL: Esc should pass in vim mode\n");
            return 1;
        }
        // Ctrl+C：V0.3.x 中文模式也透传（问题 2：Ctrl+C 复制等系统快捷键可用）；
        // vim 模式强制透传（IsVimModeKey 判定）
        keybd_event(VK_CONTROL, 0, 0, 0);  // Ctrl 按下
        const bool ctrlDown = (GetKeyState(VK_CONTROL) & 0x8000) != 0;
        if (!ctrlDown || !taishen::IsVimModeKey('C')) {
            printf("STEP7 FAIL: Ctrl+C should be vim key (ctrl=%d)\n", ctrlDown);
            keybd_event(VK_CONTROL, 0, KEYEVENTF_KEYUP, 0);
            return 1;
        }
        if (!taishen::ShouldEatKey('C', false)) {
            printf("STEP7 OK: Ctrl+C passed through (system shortcut)\n");
        } else {
            printf("STEP7 FAIL: Ctrl+C should pass through without vim mode (chinese)\n");
            keybd_event(VK_CONTROL, 0, KEYEVENTF_KEYUP, 0);
            return 1;
        }
        if (taishen::ShouldEatKey('C', true)) {
            printf("STEP7 FAIL: Ctrl+C should pass in vim mode\n");
            keybd_event(VK_CONTROL, 0, KEYEVENTF_KEYUP, 0);
            return 1;
        }
        keybd_event(VK_CONTROL, 0, KEYEVENTF_KEYUP, 0);  // Ctrl 松开
        printf("STEP7 OK vim passthrough\n");
    }

    // 小键盘符号（0.1.52）：归一化不得丢掉 + / * 的 Shift 语义
    {
        // 中文模式：小键盘 + 应输出 '+'，不能输出 '='（原 bug）
        taishen::KeyEventResult rAdd;
        const bool eatAdd = taishen::HandleKeyDown(VK_OEM_PLUS, 0, rAdd, VK_ADD);
        printf("STEP8 keypad+: eat=%d committed=%ls\n", eatAdd, rAdd.committed.c_str());
        if (!eatAdd || rAdd.committed != L"+") {
            printf("STEP8 FAIL: keypad + should commit '+', got %ls\n",
                   rAdd.committed.c_str());
            return 1;
        }
        // 小键盘 * → '*'（归一化成 '8' 后不能变数字 8 或选第 8 个候选）
        taishen::KeyEventResult rMul;
        const bool eatMul = taishen::HandleKeyDown('8', 0, rMul, VK_MULTIPLY);
        printf("STEP8 keypad*: eat=%d committed=%ls\n", eatMul, rMul.committed.c_str());
        if (!eatMul || rMul.committed != L"*") {
            printf("STEP8 FAIL: keypad * should commit '*', got %ls\n",
                   rMul.committed.c_str());
            return 1;
        }
        // 小键盘 - → '-'
        taishen::KeyEventResult rSub;
        const bool eatSub = taishen::HandleKeyDown(VK_OEM_MINUS, 0, rSub, VK_SUBTRACT);
        printf("STEP8 keypad-: eat=%d committed=%ls\n", eatSub, rSub.committed.c_str());
        if (!eatSub || rSub.committed != L"-") {
            printf("STEP8 FAIL: keypad - should commit '-', got %ls\n",
                   rSub.committed.c_str());
            return 1;
        }
        // 小键盘数字不选词（候选存在时也透传给应用输出数字）
        if (taishen::ShouldEatKey('5', false, VK_NUMPAD5)) {
            printf("STEP8 FAIL: keypad 5 should not pick candidate\n");
            return 1;
        }
        taishen::KeyEventResult rNum;
        if (taishen::HandleKeyDown('5', 0, rNum, VK_NUMPAD5)) {
            printf("STEP8 FAIL: keypad 5 should pass through\n");
            return 1;
        }
        printf("STEP8 OK keypad symbols\n");
    }

    // 英文模式：小键盘 + / - 不参与翻页（有候选时也不吞键）
    {
        keybd_event(VK_CONTROL, 0, 0, 0);
        taishen::KeyEventResult r;
        taishen::HandleKeyDown(VK_SPACE, 0, r);
        keybd_event(VK_CONTROL, 0, KEYEVENTF_KEYUP, 0);
        if (engine_get_ascii_mode() != 1) {
            printf("STEP9 FAIL: should be english mode\n");
            return 1;
        }
        if (taishen::ShouldEatKey(VK_OEM_PLUS, false, VK_ADD) ||
            taishen::ShouldEatKey(VK_OEM_MINUS, false, VK_SUBTRACT)) {
            printf("STEP9 FAIL: keypad +/- should pass through in english mode\n");
            return 1;
        }
        printf("STEP9 OK keypad no paging in english mode\n");
    }

    // 翻页优先级（0.1.52）：勾选主键盘 +/− 后，有候选时翻页优先于标点
    {
        // 切回中文模式（STEP9 结束时是英文）
        keybd_event(VK_CONTROL, 0, 0, 0);
        taishen::KeyEventResult rs;
        taishen::HandleKeyDown(VK_SPACE, 0, rs);
        keybd_event(VK_CONTROL, 0, KEYEVENTF_KEYUP, 0);
        if (engine_get_ascii_mode() != 0) {
            printf("STEP10 FAIL: should be chinese mode\n");
            return 1;
        }
        engine_reset();
        const char* pinyin = "zhonggou";
        for (const char* p = pinyin; *p != 0; ++p) {
            engine_process_key(*p);
        }
        if (engine_get_candidate_count() <= 0) {
            printf("STEP10 FAIL: need candidates, got %d\n",
                   engine_get_candidate_count());
            return 1;
        }
        taishen::KeyBindingState kb = taishen::GetKeyBindings();
        kb.pageMinusEqual = true;
        taishen::SetKeyBindings(kb);
        // 测试内置词库候选不足一页，无法断言真实换页；
        // 断言「进入翻页分支」：吞键且不提交标点（区别于未勾选时输出 '='）
        taishen::KeyEventResult rPage;
        const bool eatPage = taishen::HandleKeyDown(VK_OEM_PLUS, 0, rPage);
        printf("STEP10 page: eat=%d committed=%ls\n", eatPage,
               rPage.committed.c_str());
        if (!eatPage || !rPage.committed.empty()) {
            printf("STEP10 FAIL: + should hit page branch when pageMinusEqual on\n");
            return 1;
        }
        // 关闭后 + 恢复标点语义（输出 '='）
        kb.pageMinusEqual = false;
        taishen::SetKeyBindings(kb);
        taishen::KeyEventResult rPunct;
        const bool eatPunct = taishen::HandleKeyDown(VK_OEM_PLUS, 0, rPunct);
        printf("STEP10 kb.minus=%d eat=%d len=%d\n",
               (int)taishen::GetKeyBindings().pageMinusEqual, (int)eatPunct,
               (int)rPunct.committed.size());
        // 未勾选翻页：+ 恢复标点语义，末字符必须是 '='
        // （有候选时先上屏默认候选再接 '='，与既有标点行为一致）
        if (!eatPunct || rPunct.committed.empty() ||
            rPunct.committed.back() != L'=') {
            printf("STEP10 FAIL: + should commit '=' when paging off\n");
            return 1;
        }
        engine_reset();
        printf("STEP10 OK page key priority\n");
    }

    // 输入进行中的数字键：候选越界必须吞键（bug 复现：组词翻页到末页后按
    // 数字选字，数字透传给宿主被写进组合串，出现「12」这类无来源数字）
    {
        if (engine_get_ascii_mode() != 0) {
            keybd_event(VK_CONTROL, 0, 0, 0);
            taishen::KeyEventResult rs;
            taishen::HandleKeyDown(VK_SPACE, 0, rs);
            keybd_event(VK_CONTROL, 0, KEYEVENTF_KEYUP, 0);
        }
        // 场景 A：有拼音、候选不足 9 个 → 按 9 越界，必须吞且不提交
        engine_reset();
        for (const char* p = "vvvvv"; *p != 0; ++p) {
            engine_process_key(*p);
        }
        const int count = engine_get_candidate_count();
        const int pinyinLen = engine_get_pinyin_str(nullptr, 0);
        printf("STEP11 state: cands=%d pinyin_len=%d\n", count, pinyinLen);
        if (pinyinLen <= 1) {
            printf("STEP11 FAIL: setup pinyin failed\n");
            return 1;
        }
        if (count < 9) {
            if (!taishen::ShouldEatKey('9', false, '9')) {
                printf("STEP11 FAIL: out-of-range digit should be eaten while input active\n");
                return 1;
            }
            taishen::KeyEventResult r;
            const bool eat = taishen::HandleKeyDown('9', 0, r);
            printf("STEP11 out-of-range 9: eat=%d committed=%ls\n", (int)eat,
                   r.committed.c_str());
            if (!eat || !r.committed.empty()) {
                printf("STEP11 FAIL: out-of-range digit leaked to host\n");
                return 1;
            }
            // 引擎状态不被破坏：拼音仍在，可以继续输入
            if (engine_get_pinyin_str(nullptr, 0) <= 1) {
                printf("STEP11 FAIL: pinyin should stay after ignored digit\n");
                return 1;
            }
        }
        // 场景 B（回归）：无拼音无候选时数字仍透传，不影响正常输入数字
        engine_reset();
        taishen::KeyEventResult rPlain;
        const bool eatPlain = taishen::HandleKeyDown('5', 0, rPlain);
        printf("STEP11 idle digit: eat=%d\n", (int)eatPlain);
        if (eatPlain) {
            printf("STEP11 FAIL: digit without pinyin should pass through\n");
            return 1;
        }
        printf("STEP11 OK out-of-range digit eaten, idle digit passes\n");
    }

    // STEP12：组词全程翻页/选字不得带出数字（Eric 复现："多翻几页冒出 12"）
    // 完整模拟：输入多音节 → 反复翻页触发组词 → 数字选首字 → 继续翻页选字，
    // 每一步检查 committed / 拼音串里是否混入 ASCII 数字。
    {
        engine_reset();
        for (const char* p = "zhongguo"; *p != 0; ++p) {
            engine_process_key(*p);
        }
        printf("STEP12 setup: cands=%d pinyin_len=%d\n",
               engine_get_candidate_count(), engine_get_pinyin_str(nullptr, 0));

        auto hasDigit = [](const std::wstring& w) {
            for (wchar_t c : w) {
                if (c >= L'0' && c <= L'9') return true;
            }
            return false;
        };
        auto pinyinHasDigit = []() {
            const int need = engine_get_pinyin_str(nullptr, 0);
            if (need <= 1) return false;
            std::string py(static_cast<size_t>(need), '\0');
            engine_get_pinyin_str(py.data(), need);
            for (char c : py) {
                if (c >= '0' && c <= '9') return true;
            }
            return false;
        };

        // 打印组合串状态的小工具
        auto compRemain = []() {
            const int need = engine_compose_remaining(nullptr, 0);
            if (need <= 1) return std::string();
            std::string s(static_cast<size_t>(need), '\0');
            engine_compose_remaining(s.data(), need);
            return s;
        };
        auto compCur = []() {
            const int need = engine_compose_info(nullptr, 0);
            if (need <= 1) return std::string();
            std::string s(static_cast<size_t>(need), '\0');
            engine_compose_info(s.data(), need);
            return s;
        };
        auto compPrefix = []() {
            const int need = engine_compose_prefix(nullptr, 0);
            if (need <= 1) return std::string();
            std::string s(static_cast<size_t>(need), '\0');
            engine_compose_prefix(s.data(), need);
            return s;
        };

        bool leaked = false;
        // 第一段：不选字，连续翻页 8 次（跨过页边界与进组词阈值）
        for (int i = 0; i < 8; ++i) {
            taishen::KeyEventResult r;
            const bool eat = taishen::HandleKeyDown(VK_NEXT, 0, r);
            const bool eatTest = taishen::ShouldEatKey(VK_NEXT, false, VK_NEXT);
            if (hasDigit(r.committed)) leaked = true;
            printf("STEP12 page[%d] eat=%d eatTest=%d compose=%d page=%d cands=%d "
                   "committed_len=%zu cur=[%s] remain=[%s] pref=[%s]\n",
                   i, (int)eat, (int)eatTest, engine_in_compose(),
                   engine_get_current_page(), engine_get_candidate_count(),
                   r.committed.size(), compCur().c_str(), compRemain().c_str(),
                   compPrefix().c_str());
        }
        // 第二段：核心断言——组词首位必须是单字；按 1 后组词必须继续。
        // 修复前的失败态：首位是吃掉全部剩余音节的整词，按 1 后 compose=0、
        // 拼音清空，后续数字全部透传宿主（正文冒出「12」）。
        {
            char first[128] = {0};
            engine_get_candidate_at(0, first, sizeof(first));
            const int firstChars =
                MultiByteToWideChar(CP_UTF8, 0, first, -1, nullptr, 0) - 1;
            printf("STEP12 compose first=[%s] chars=%d compose=%d\n", first,
                   firstChars, engine_in_compose());
            if (engine_in_compose() != 1) {
                printf("STEP12 FAIL: expected compose mode after 3 pages\n");
                return 1;
            }
            if (firstChars != 1) {
                printf("STEP12 FAIL: compose first candidate must be single char, "
                       "got [%s]\n", first);
                return 1;
            }
            taishen::KeyEventResult r;
            const bool eat = taishen::HandleKeyDown('1', 0, r);
            printf("STEP12 pick1 eat=%d compose=%d committed_len=%zu remain=[%s]\n",
                   (int)eat, engine_in_compose(), r.committed.size(),
                   compRemain().c_str());
            if (!eat || r.committed.size() != 1) {
                printf("STEP12 FAIL: picking first candidate should commit one char\n");
                return 1;
            }
            if (engine_in_compose() != 1) {
                printf("STEP12 FAIL: picking first char must keep compose alive "
                       "(this is the 12-digit root cause), compose=%d\n",
                       engine_in_compose());
                return 1;
            }
        }
        // 组词中按 0 与小键盘数字：不参与选词，但必须被吞（审计发现的透传路径）
        {
            const struct {
                int vk;
                int raw;
                const char* name;
            } digits[] = {{'0', '0', "main-0"}, {'5', VK_NUMPAD5, "numpad-5"}};
            for (const auto& d : digits) {
                taishen::KeyEventResult rd;
                const bool eatTest = taishen::ShouldEatKey(d.vk, false, d.raw);
                const bool eat = taishen::HandleKeyDown(d.vk, 0, rd, d.raw);
                printf("STEP12 digit %s eat=%d eatTest=%d compose=%d\n", d.name,
                       (int)eat, (int)eatTest, engine_in_compose());
                if (!eat || !eatTest) {
                    printf("STEP12 FAIL: %s must be eaten while composing\n", d.name);
                    return 1;
                }
            }
        }
        printf("STEP12 pinyin_has_digit=%d leaked=%d\n", (int)pinyinHasDigit(),
               (int)leaked);
        if (leaked) {
            printf("STEP12 FAIL: digit leaked during compose paging\n");
            return 1;
        }
        printf("STEP12 OK compose first is single char, digits swallowed\n");
    }

    printf("ALL TESTS PASSED\n");
    engine_destroy();
    return 0;
}
