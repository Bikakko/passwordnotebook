//! 主窗口:锁定 / 创建 / 已解锁 / 回收站 四种形态共用同一个顶层窗口。

use std::ffi::c_void;
use std::path::Path;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::UI::Controls::NMHDR;
use windows::Win32::UI::WindowsAndMessaging::MSG;

use zeroize::Zeroizing;

use crate::model::Entry;
use crate::strength;
use crate::vault::VaultService;
use crate::vaultfile::VaultFile;

use super::app::{self, Mode};
use super::sys::*;
use super::timefmt;
use super::ui;
use super::{
    clipboard, dialog, dlg_editor, dlg_generator, dlg_input, dlg_recovery, dlg_settings, dlg_taxonomy,
    dlg_transfer, dpapi, hello, idle,
};

/// 列表行高度(基准 96 DPI 下的高度;系统原本约为 24px,增加 20%~25% 至 30px,
/// 同步增大上下内边距使行间距更舒适)。
pub const LIST_ROW_HEIGHT: i32 = 30;

/// 加强网格线的颜色(比系统默认淡淡的灰色更显眼、更清晰)。
/// COLORREF 为 0x00BBGGRR:RGB(197, 202, 211) -> 0x00D3CAC5
pub const GRIDLINE_COLOR: u32 = 0x00D3_CAC5;

/// 列表列定义:(列标题, 96 DPI 基准逻辑像素宽度, 默认百分比分配)
pub type ListColDef = (&'static str, i32, i32);

/// 主列表列配置
pub const MAIN_LIST_COLUMNS: &[ListColDef] = &[
    ("标题", 147, 17),
    ("用户名", 113, 13),
    ("网址", 170, 21),
    ("分类", 80, 9),
    ("标签", 160, 20),
    ("更新时间", 150, 20),
];

/// 回收站列表列配置
pub const BIN_LIST_COLUMNS: &[ListColDef] = &[
    ("标题", 220, 28),
    ("用户名", 170, 20),
    ("分类", 120, 14),
    ("删除时间", 170, 20),
    ("保留", 120, 18),
];

// ---------- 控件 ID ----------
const ID_UNLOCK_TITLE: usize = 2001;
const ID_UNLOCK_HINT: usize = 2002;
const ID_UNLOCK_PATH_LABEL: usize = 2003;
/// 只读显示数据库位置(固定路径,不可选)。
const ID_UNLOCK_PATH: usize = 2004;
const ID_UNLOCK_PW_LABEL: usize = 2006;
const ID_UNLOCK_PW: usize = 2007;
const ID_UNLOCK_SHOW: usize = 2008;
const ID_UNLOCK_BTN: usize = 2009;
pub const ID_HELLO_BTN: usize = 2010;
const ID_FORGOT_BTN: usize = 2011;
pub const ID_GOTO_CREATE_BTN: usize = 2012;
const ID_UNLOCK_ERROR: usize = 2013;

const ID_CREATE_TITLE: usize = 2101;
const ID_CREATE_HINT: usize = 2102;
const ID_CREATE_PATH_LABEL: usize = 2103;
/// 只读显示数据库位置。
const ID_CREATE_PATH: usize = 2104;
const ID_CREATE_PW_LABEL: usize = 2106;
const ID_CREATE_PW: usize = 2107;
const ID_CREATE_SHOW: usize = 2108;
const ID_CREATE_PW2_LABEL: usize = 2109;
const ID_CREATE_PW2: usize = 2110;
const ID_CREATE_STRENGTH: usize = 2111;
const ID_CREATE_BTN: usize = 2112;
const ID_CREATE_ERROR: usize = 2114;

const ID_GEN_BTN: usize = 2204;
const ID_BIN_BTN: usize = 2205;
const ID_SETTINGS_BTN: usize = 2206;
const ID_LOCK_BTN: usize = 2207;
const ID_SEARCH: usize = 2208;
const ID_CAT_TABS: usize = 2210;

/// 分类标签页的下标约定:0 = 全部,1 起对应 known_categories()。
/// 未分类的条目直接显示在「全部」里,不再单独占一个页签。
const TAB_ALL: i32 = 0;
const TAB_CATEGORY_BASE: i32 = 1;
const ID_TAG_LABEL: usize = 2211;
const ID_TAG_LIST: usize = 2212;
const ID_LIST: usize = 2213;
const ID_SORT_COMBO: usize = 2217;
const ID_STATUS: usize = 2218;
const ID_TAXONOMY_BTN: usize = 2219;
/// 导入 / 导出。
const ID_TRANSFER_BTN: usize = 2220;
/// 「只看收藏」开关(可叠加在分类 / 标签 / 搜索之上)。
const ID_FAV_ONLY_BTN: usize = 2221;
/// 工具栏「新建」按钮(与右键菜单共用 `new_entry`)。
const ID_NEW_BTN: usize = 2222;
/// 主列表没有可显示的条目时的空状态提示。
const ID_EMPTY_HINT: usize = 2223;

const ID_BIN_TITLE: usize = 2301;
const ID_BIN_HINT: usize = 2302;
const ID_BIN_LIST: usize = 2303;
const ID_BIN_RESTORE_BTN: usize = 2304;
const ID_BIN_PURGE_BTN: usize = 2305;
const ID_BIN_EMPTY_BTN: usize = 2306;
const ID_BIN_BACK_BTN: usize = 2307;
/// 回收站为空的提示(没有条目时才显示)。
const ID_BIN_EMPTY_HINT: usize = 2308;

const TIMER_CLIPBOARD: usize = 2;
const TIMER_IDLE: usize = 3;
const TIMER_HELLO: usize = 4;
/// 一次性短定时器:等后台的 Hello 可用性检测出结果,刷新按钮后即停。
const TIMER_HELLO_PROBE: usize = 5;
const TIMER_IDLE_PERIOD: u32 = 5000;

const ALL_CATEGORIES: &str = "全部";
const UNCATEGORIZED: &str = "未分类";
const ALL_TAGS: &str = "全部标签";
const FAV_ONLY_LABEL: &str = "只看收藏";

/// 主窗口的全部控件句柄与运行期状态。
pub struct MainUi {
    pub unlock_title: HWND,
    pub unlock_hint: HWND,
    pub unlock_path_label: HWND,
    /// 只读显示数据库位置。
    pub unlock_path: HWND,
    pub unlock_pw_label: HWND,
    pub unlock_pw: HWND,
    pub unlock_show: HWND,
    pub unlock_btn: HWND,
    pub hello_btn: HWND,
    pub forgot_btn: HWND,
    pub goto_create_btn: HWND,
    pub unlock_error: HWND,

    pub create_title: HWND,
    pub create_hint: HWND,
    pub create_path_label: HWND,
    /// 只读显示数据库位置。
    pub create_path: HWND,
    pub create_pw_label: HWND,
    pub create_pw: HWND,
    pub create_show: HWND,
    pub create_pw2_label: HWND,
    pub create_pw2: HWND,
    pub create_strength: HWND,
    pub create_btn: HWND,
    pub create_error: HWND,

    /// 工具栏「新建」:不依赖列表空白区,任何时候都点得到。
    pub new_btn: HWND,
    pub gen_btn: HWND,
    pub bin_btn: HWND,
    pub taxonomy_btn: HWND,
    pub transfer_btn: HWND,
    /// 「只看收藏」开关:和分类 / 标签 / 搜索是叠加关系,不是页签。
    pub fav_only_btn: HWND,
    pub settings_btn: HWND,
    pub lock_btn: HWND,
    pub search: HWND,
    pub cat_tabs: HWND,
    pub tag_label: HWND,
    pub tag_list: HWND,
    pub list: HWND,
    pub sort_combo: HWND,
    pub status: HWND,
    /// 主列表为空时居中显示的引导文字。
    pub empty_hint: HWND,

    pub bin_title: HWND,
    pub bin_hint: HWND,
    pub bin_list: HWND,
    pub bin_restore_btn: HWND,
    pub bin_purge_btn: HWND,
    pub bin_empty_btn: HWND,
    pub bin_back_btn: HWND,
    /// 回收站为空时居中显示的提示文字。
    pub bin_empty_hint: HWND,

    /// 条目列表当前显示顺序对应的条目 id。
    rows: Vec<String>,
    /// 回收站列表当前显示顺序对应的条目 id。
    bin_rows: Vec<String>,
    /// 剪贴板自动清空:到期时间与期望内容。
    /// 内容可能是密码,用 `Zeroizing` 让它到期后自动抹掉。
    clipboard_deadline: Option<(Instant, Zeroizing<String>)>,
    /// 设备是否支持 Windows Hello(启动时探测一次)。
    hello_available: bool,
    /// 正在等待指纹验证的数据密钥(验证通过后才用它解锁)。
    /// 用 `Zeroizing`:验证失败或界面切走时会被抹掉,而不是留在堆上。
    pending_hello_dek: Option<Zeroizing<Vec<u8>>>,
}

impl MainUi {
    pub fn new() -> Self {
        let zero = HWND::default();
        Self {
            unlock_title: zero,
            unlock_hint: zero,
            unlock_path_label: zero,
            unlock_path: zero,
            unlock_pw_label: zero,
            unlock_pw: zero,
            unlock_show: zero,
            unlock_btn: zero,
            hello_btn: zero,
            forgot_btn: zero,
            goto_create_btn: zero,
            unlock_error: zero,
            create_title: zero,
            create_hint: zero,
            create_path_label: zero,
            create_path: zero,
            create_pw_label: zero,
            create_pw: zero,
            create_show: zero,
            create_pw2_label: zero,
            create_pw2: zero,
            create_strength: zero,
            create_btn: zero,
            create_error: zero,
            new_btn: zero,
            gen_btn: zero,
            bin_btn: zero,
            taxonomy_btn: zero,
            transfer_btn: zero,
            fav_only_btn: zero,
            settings_btn: zero,
            lock_btn: zero,
            search: zero,
            cat_tabs: zero,
            tag_label: zero,
            tag_list: zero,
            list: zero,
            sort_combo: zero,
            status: zero,
            empty_hint: zero,
            bin_title: zero,
            bin_hint: zero,
            bin_list: zero,
            bin_restore_btn: zero,
            bin_purge_btn: zero,
            bin_empty_btn: zero,
            bin_back_btn: zero,
            bin_empty_hint: zero,
            rows: Vec::new(),
            bin_rows: Vec::new(),
            clipboard_deadline: None,
            hello_available: false,
            pending_hello_dek: None,
        }
    }
}

fn st(hwnd: HWND) -> &'static mut MainUi {
    unsafe { ui::state_ref::<MainUi>(hwnd) }
}

/// 主窗口全部控件的 ID(用于统一设置字体)。
const ALL_CONTROL_IDS: &[usize] = &[
    ID_UNLOCK_TITLE, ID_UNLOCK_HINT, ID_UNLOCK_PATH_LABEL, ID_UNLOCK_PATH, ID_UNLOCK_PW_LABEL,
    ID_UNLOCK_PW, ID_UNLOCK_SHOW, ID_UNLOCK_BTN, ID_HELLO_BTN, ID_FORGOT_BTN, ID_GOTO_CREATE_BTN,
    ID_UNLOCK_ERROR,
    ID_CREATE_TITLE, ID_CREATE_HINT, ID_CREATE_PATH_LABEL, ID_CREATE_PATH, ID_CREATE_PW_LABEL,
    ID_CREATE_PW, ID_CREATE_SHOW, ID_CREATE_PW2_LABEL, ID_CREATE_PW2, ID_CREATE_STRENGTH,
    ID_CREATE_BTN, ID_CREATE_ERROR,
    ID_GEN_BTN, ID_BIN_BTN, ID_TAXONOMY_BTN, ID_TRANSFER_BTN, ID_FAV_ONLY_BTN, ID_NEW_BTN,
    ID_SETTINGS_BTN, ID_LOCK_BTN,
    ID_SEARCH, ID_CAT_TABS, ID_TAG_LABEL, ID_TAG_LIST, ID_LIST, ID_SORT_COMBO,
    ID_STATUS, ID_EMPTY_HINT,
    ID_BIN_TITLE, ID_BIN_HINT, ID_BIN_LIST, ID_BIN_RESTORE_BTN, ID_BIN_PURGE_BTN,
    ID_BIN_EMPTY_BTN, ID_BIN_BACK_BTN, ID_BIN_EMPTY_HINT,
];

/// 给所有控件套用当前字体(DPI 变化时重新调用)。
pub fn apply_fonts(hwnd: HWND) {
    let font = app::state().font;
    let bold = app::state().font_bold;
    ui::apply_font_to(hwnd, ALL_CONTROL_IDS, font);

    let s = st(hwnd);
    for control in [s.unlock_title, s.create_title, s.bin_title] {
        ui::send_msg(control, WM_SETFONT, bold.0 as usize, 1);
    }
    // 按钮与分类页签用粗体,列表列头也加粗:动作/标题类信息与正文拉开层级。
    ui::apply_bold_actions(hwnd, bold);
    ui::listview_bold_header(s.list, bold);
    ui::listview_bold_header(s.bin_list, bold);

    if s.list.0 != std::ptr::null_mut() {
        ui::listview_set_row_height(s.list, scale(LIST_ROW_HEIGHT));
    }
    if s.bin_list.0 != std::ptr::null_mut() {
        ui::listview_set_row_height(s.bin_list, scale(LIST_ROW_HEIGHT));
    }
}

/// 静态文本统一画成透明背景;错误提示用红色。
///
/// 颜色是 COLORREF(0x00BBGGRR):#1F2430 → 0x0030241F,#C0392B → 0x002B39C0。
pub fn on_ctlcolor_static(hwnd: HWND, hdc_raw: usize, control_raw: isize) -> isize {
    use windows::Win32::Graphics::Gdi::HDC;

    let hdc = HDC(hdc_raw as *mut std::ffi::c_void);
    let control = HWND(control_raw as *mut std::ffi::c_void);

    let s = st(hwnd);
    let is_error = control == s.unlock_error || control == s.create_error;
    let color = if is_error { 0x002B39C0 } else { 0x0030241F };

    ui::paint_static_label(hdc, color)
}

fn text(parent: HWND, s: &str, id: usize) -> HWND {
    ui::create_window("STATIC", s, WS_CHILD | SS_LEFT, 0, parent, id, 0, 0, 10, 10)
}

/// 空状态提示:文本在整行宽度里居中。
fn center_text(parent: HWND, s: &str, id: usize) -> HWND {
    ui::create_window("STATIC", s, WS_CHILD | SS_CENTER, 0, parent, id, 0, 0, 10, 10)
}

fn button(parent: HWND, s: &str, style: u32, id: usize) -> HWND {
    ui::create_window("BUTTON", s, WS_CHILD | WS_TABSTOP | style, 0, parent, id, 0, 0, 10, 10)
}

fn edit(parent: HWND, style: u32, id: usize) -> HWND {
    ui::create_window(
        "EDIT",
        "",
        WS_CHILD | WS_BORDER | WS_TABSTOP | ES_LEFT | style,
        WS_EX_CLIENTEDGE,
        parent,
        id,
        0,
        0,
        10,
        10,
    )
}

fn checkbox(parent: HWND, s: &str, id: usize) -> HWND {
    ui::create_window("BUTTON", s, WS_CHILD | WS_TABSTOP | BS_AUTOCHECKBOX, 0, parent, id, 0, 0, 10, 10)
}

fn listbox(parent: HWND, id: usize) -> HWND {
    ui::create_window(
        "LISTBOX",
        "",
        WS_CHILD | WS_BORDER | WS_VSCROLL | WS_TABSTOP | LBS_NOTIFY | LBS_NOINTEGRALHEIGHT,
        0,
        parent,
        id,
        0,
        0,
        10,
        10,
    )
}

fn listview(parent: HWND, id: usize) -> HWND {
    ui::create_window(
        "SysListView32",
        "",
        WS_CHILD | WS_BORDER | WS_VSCROLL | WS_TABSTOP | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS,
        WS_EX_CLIENTEDGE,
        parent,
        id,
        0,
        0,
        10,
        10,
    )
}

/// `WM_CREATE`:创建全部控件。
pub fn on_create(hwnd: HWND, lparam: LPARAM) {
    let state_ptr = unsafe { ui::create_param(lparam) } as *mut MainUi;
    ui::set_user_data(hwnd, state_ptr as *mut c_void);
    let s = unsafe { &mut *state_ptr };

    s.unlock_title = text(hwnd, "解锁密码本", ID_UNLOCK_TITLE);
    s.unlock_hint = text(
        hwnd,
        "输入登录密码解锁。",
        ID_UNLOCK_HINT,
    );
    s.unlock_path_label = text(hwnd, "密码本文件", ID_UNLOCK_PATH_LABEL);
    s.unlock_path = text(hwnd, "", ID_UNLOCK_PATH);
    s.unlock_pw_label = text(hwnd, "登录密码", ID_UNLOCK_PW_LABEL);
    s.unlock_pw = edit(hwnd, ES_PASSWORD, ID_UNLOCK_PW);
    s.unlock_show = checkbox(hwnd, "显示密码", ID_UNLOCK_SHOW);
    s.unlock_btn = button(hwnd, "解锁", BS_DEFPUSHBUTTON, ID_UNLOCK_BTN);
    s.hello_btn = button(hwnd, "使用 Windows Hello 解锁", BS_PUSHBUTTON, ID_HELLO_BTN);
    s.forgot_btn = button(hwnd, "忘记登录密码？", BS_PUSHBUTTON, ID_FORGOT_BTN);
    s.goto_create_btn = button(hwnd, "创建新密码本", BS_PUSHBUTTON, ID_GOTO_CREATE_BTN);
    s.unlock_error = text(hwnd, "", ID_UNLOCK_ERROR);

    s.create_title = text(hwnd, "创建新密码本", ID_CREATE_TITLE);
    s.create_hint = text(
        hwnd,
        "密码本用登录密码加密，拷到任意电脑都能打开。登录密码不会被保存，请务必牢记。",
        ID_CREATE_HINT,
    );
    s.create_path_label = text(hwnd, "密码本文件", ID_CREATE_PATH_LABEL);
    s.create_path = text(hwnd, "", ID_CREATE_PATH);
    s.create_pw_label = text(hwnd, "登录密码", ID_CREATE_PW_LABEL);
    s.create_pw = edit(hwnd, ES_PASSWORD, ID_CREATE_PW);
    s.create_show = checkbox(hwnd, "显示登录密码", ID_CREATE_SHOW);
    s.create_pw2_label = text(hwnd, "确认登录密码", ID_CREATE_PW2_LABEL);
    s.create_pw2 = edit(hwnd, ES_PASSWORD, ID_CREATE_PW2);
    s.create_strength = text(hwnd, "", ID_CREATE_STRENGTH);
    s.create_btn = button(hwnd, "创建并开始使用", BS_DEFPUSHBUTTON, ID_CREATE_BTN);
    s.create_error = text(hwnd, "", ID_CREATE_ERROR);

    s.new_btn = button(hwnd, "新建", BS_PUSHBUTTON, ID_NEW_BTN);
    s.gen_btn = button(hwnd, "生成密码", BS_PUSHBUTTON, ID_GEN_BTN);
    s.bin_btn = button(hwnd, "回收站", BS_PUSHBUTTON, ID_BIN_BTN);
    s.taxonomy_btn = button(hwnd, "分类与标签", BS_PUSHBUTTON, ID_TAXONOMY_BTN);
    s.transfer_btn = button(hwnd, "导入/导出", BS_PUSHBUTTON, ID_TRANSFER_BTN);
    s.fav_only_btn = button(hwnd, FAV_ONLY_LABEL, BS_AUTOCHECKBOX, ID_FAV_ONLY_BTN);
    s.settings_btn = button(hwnd, "设置", BS_PUSHBUTTON, ID_SETTINGS_BTN);
    s.lock_btn = button(hwnd, "锁定", BS_PUSHBUTTON, ID_LOCK_BTN);
    s.search = edit(hwnd, ES_AUTOHSCROLL, ID_SEARCH);
    ui::register_tab_strip_class();
    s.cat_tabs = ui::create_window(
        "PnbTabStrip",
        "",
        WS_CHILD | WS_TABSTOP | TCS_MULTILINE,
        0,
        hwnd,
        ID_CAT_TABS,
        0,
        0,
        10,
        10,
    );
    s.tag_label = text(hwnd, "标签", ID_TAG_LABEL);
    s.tag_list = listbox(hwnd, ID_TAG_LIST);
    s.list = listview(hwnd, ID_LIST);
    s.sort_combo = ui::create_window(
        "COMBOBOX",
        "",
        WS_CHILD | WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST,
        0,
        hwnd,
        ID_SORT_COMBO,
        0,
        0,
        10,
        10,
    );
    s.status = text(hwnd, "", ID_STATUS);
    // 空状态:默认隐藏,列表为空时由 refresh_list / refresh_bin 显示出来。
    s.empty_hint = center_text(hwnd, "还没有条目。点「新建」开始记录。", ID_EMPTY_HINT);

    ui::listview_set_extended_style(s.list, LVS_EX_FULLROWSELECT | LVS_EX_GRIDLINES);
    for (i, &(title, width, _)) in MAIN_LIST_COLUMNS.iter().enumerate() {
        ui::listview_add_column(s.list, i as i32, scale(width), title);
    }

    s.bin_title = text(hwnd, "回收站", ID_BIN_TITLE);
    s.bin_hint = text(
        hwnd,
        "删除的条目先放进回收站，可随时恢复；超过保留期会在下次解锁时彻底删除。",
        ID_BIN_HINT,
    );
    s.bin_list = listview(hwnd, ID_BIN_LIST);
    s.bin_restore_btn = button(hwnd, "恢复", BS_PUSHBUTTON, ID_BIN_RESTORE_BTN);
    s.bin_purge_btn = button(hwnd, "彻底删除", BS_PUSHBUTTON, ID_BIN_PURGE_BTN);
    s.bin_empty_btn = button(hwnd, "清空回收站", BS_PUSHBUTTON, ID_BIN_EMPTY_BTN);
    s.bin_back_btn = button(hwnd, "返回列表", BS_PUSHBUTTON, ID_BIN_BACK_BTN);
    // 空状态:默认隐藏,由 refresh_bin 按回收站条数显示。
    s.bin_empty_hint = center_text(hwnd, "回收站是空的。", ID_BIN_EMPTY_HINT);

    ui::listview_set_extended_style(s.bin_list, LVS_EX_FULLROWSELECT | LVS_EX_GRIDLINES);
    for (i, &(title, width, _)) in BIN_LIST_COLUMNS.iter().enumerate() {
        ui::listview_add_column(s.bin_list, i as i32, scale(width), title);
    }

    ui::combo_add(s.sort_combo, "更新时间");
    ui::combo_add(s.sort_combo, "标题");
    ui::combo_add(s.sort_combo, "分类");
    ui::combo_set_index(s.sort_combo, 0);

    // 统一套用界面字体(标题用半粗体)。
    apply_fonts(hwnd);

    // 可用性检测要问系统、可能要几百毫秒:放后台跑,先让窗口正常起来;
    // 结果就绪后由 TIMER_HELLO_PROBE 刷新 Hello 按钮的可见性。
    hello::probe_availability();
    s.hello_available = hello::availability().unwrap_or(false);

    ui::set_timer(hwnd, TIMER_IDLE, TIMER_IDLE_PERIOD);
    ui::set_timer(hwnd, TIMER_HELLO_PROBE, 200);

    refresh_filters(hwnd);
    refresh_list(hwnd);
    apply_mode(hwnd);
}

// ---------- 形态切换 ----------

pub fn apply_mode(hwnd: HWND) {
    let mode = app::state().mode;
    let s = st(hwnd);

    let unlock = mode == Mode::Unlock;
    let create = mode == Mode::Create;
    let main = mode == Mode::Unlocked;
    let bin = mode == Mode::Bin;

    for c in [
        s.unlock_title, s.unlock_hint, s.unlock_path_label, s.unlock_path, s.unlock_pw_label,
        s.unlock_pw, s.unlock_show, s.unlock_btn, s.forgot_btn, s.unlock_error,
    ] {
        ui::set_visible(c, unlock);
    }

    // 「创建新密码本」只在数据库文件确实不在时才有意义。文件在时点进去、把表单
    // 填完也会被 create_vault 以「数据库已存在」挡回来,是条死路。
    ui::set_visible(s.goto_create_btn, unlock && !crate::paths::vault_exists());
    for c in [
        s.create_title, s.create_hint, s.create_path_label, s.create_path, s.create_pw_label,
        s.create_pw, s.create_show, s.create_pw2_label, s.create_pw2, s.create_strength,
        s.create_btn, s.create_error,
    ] {
        ui::set_visible(c, create);
    }
    for c in [
        s.new_btn, s.gen_btn, s.bin_btn, s.taxonomy_btn, s.transfer_btn, s.settings_btn, s.lock_btn,
        s.search, s.cat_tabs, s.fav_only_btn, s.tag_label, s.tag_list, s.list, s.sort_combo,
        s.status,
    ] {
        ui::set_visible(c, main);
    }
    for c in [
        s.bin_title, s.bin_hint, s.bin_list, s.bin_restore_btn, s.bin_purge_btn, s.bin_empty_btn,
        s.bin_back_btn,
    ] {
        ui::set_visible(c, bin);
    }
    // 空状态跟着数据走,不能按形态无条件显示:apply_mode 总是在 refresh_* 之后调用,
    // 无条件显示会把 refresh_* 刚算好的结论覆盖掉(列表里明明有条目却盖着「还没有条目」)。
    ui::set_visible(s.empty_hint, main && s.rows.is_empty());
    ui::set_visible(s.bin_empty_hint, bin && s.bin_rows.is_empty());

    // Windows Hello 按钮只在解锁界面出现(且要有可用的免密缓存)。
    // 注意:必须在**所有**形态下显式设置它 —— 只写 `if unlock` 的话,
    // 从解锁界面切走时它会保持可见,一路带到密码管理界面上。
    if unlock {
        let cached = has_quick_unlock_cache();
        ui::set_visible(s.hello_btn, s.hello_available && cached);
    } else {
        ui::set_visible(s.hello_btn, false);
        // 顺带把可能还在轮询的 Hello 验证停掉,别让它跨形态继续生效。
        ui::kill_timer(hwnd, TIMER_HELLO);
        s.pending_hello_dek = None;
    }

    // 数据库位置只读显示。正常情况下只给文件名,避免界面上出现绝对路径。
    let display = crate::paths::vault_display_path();
    ui::set_text(s.unlock_path, &display);
    ui::set_text(s.create_path, &display);

    ui::set_text(hwnd, &super::app_title());
    layout(hwnd);

    // 进入解锁界面时把焦点放到密码框,方便直接输入。
    if unlock {
        ui::set_focus(st(hwnd).unlock_pw);
    } else if create {
        ui::set_focus(st(hwnd).create_path);
    }
}

/// 按当前 DPI 缩放逻辑像素(96 DPI 逻辑像素 -> 当前物理像素)。
fn scale(v: i32) -> i32 {
    let dpi = app::state().dpi as f32;
    (v as f32 * dpi / 96.0).round() as i32
}

/// 逆向 DPI 缩放(当前物理像素 -> 96 DPI 基准逻辑像素)。
fn unscale(v: i32) -> i32 {
    let dpi = app::state().dpi.max(96) as f32;
    ((v as f32 * 96.0) / dpi).round() as i32
}

fn place(hwnd: HWND, x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::MoveWindow(
            hwnd,
            x,
            y,
            w.max(1),
            h.max(1),
            true,
        );
    }
}

pub fn layout(hwnd: HWND) {
    layout_inner(hwnd);
    // 排完版立刻强制整窗(含子控件)重画。从最大化还原时窗口缩小,
    // 新暴露出来的区域必须马上擦掉,否则会残留旧画面的碎片。
    ui::redraw_all(hwnd);
}

fn layout_inner(hwnd: HWND) {
    let s = st(hwnd);
    let (cw, ch) = ui::client_size(hwnd);

    let mode = app::state().mode;

    // 表单形态:面板限宽并居中,避免最大化后控件被拉得又扁又长。
    if mode == Mode::Unlock || mode == Mode::Create {
        let content_h = if mode == Mode::Unlock { 500 } else { 560 };
        let panel_w = scale(560).min((cw - scale(48)).max(scale(320)));
        let x = ((cw - panel_w) / 2).max(scale(16));
        let top = ((ch - scale(content_h)) / 2).clamp(scale(16), scale(140));
        layout_form(s, x, top, panel_w, mode == Mode::Create);
        return;
    }

    let pad = scale(20);

    if mode == Mode::Bin {
        let w = cw - pad * 2;
        place(s.bin_title, pad, pad, w, scale(30));
        place(s.bin_hint, pad, pad + scale(34), w, scale(44));
        let list_h = (ch - pad * 2 - scale(82) - scale(48)).max(scale(80));
        place(s.bin_list, pad, pad + scale(82), w, list_h);
        place(
            s.bin_empty_hint,
            pad,
            pad + scale(82) + list_h / 2 - scale(12),
            w,
            scale(24),
        );
        let bin_percents: Vec<i32> = BIN_LIST_COLUMNS.iter().map(|c| c.2).collect();
        set_list_columns(s.bin_list, w, &bin_percents);
        let by = ch - pad - scale(36);
        place(s.bin_restore_btn, pad, by, scale(110), scale(34));
        place(s.bin_purge_btn, pad + scale(122), by, scale(120), scale(34));
        place(s.bin_empty_btn, pad + scale(254), by, scale(130), scale(34));
        place(s.bin_back_btn, pad + scale(396), by, scale(120), scale(34));
        return;
    }

    // 已解锁形态
    let margin = scale(10);
    let btn_w = scale(88);
    let btn_h = scale(30);
    let gap = scale(6);
    let mut x = margin;
    for b in [s.new_btn, s.gen_btn, s.bin_btn, s.taxonomy_btn, s.transfer_btn] {
        place(b, x, margin, btn_w, btn_h);
        x += btn_w + gap;
    }
    let right = cw - margin - btn_w;
    place(s.lock_btn, right, margin, btn_w, btn_h);
    place(s.settings_btn, right - btn_w - gap, margin, btn_w, btn_h);

    // 排序下拉移入工具栏(左下角那排操作按钮已删除)
    let sort_w = scale(140);
    place(
        s.sort_combo,
        right - btn_w - gap - sort_w - scale(16),
        margin,
        sort_w,
        btn_h,
    );

    let search_y = margin + btn_h + scale(8);
    place(s.search, margin, search_y, cw - margin * 2, scale(28));

    let top = search_y + scale(28) + scale(8);
    let status_h = scale(22);
    let body_h = (ch - top - status_h - margin).max(scale(80));

    // 左侧:标签
    let pane_w = scale(190);
    place(s.tag_label, margin, top, pane_w, scale(20));
    place(
        s.tag_list,
        margin,
        top + scale(22),
        pane_w,
        (body_h - scale(22)).max(scale(40)),
    );

    // 右侧:分类标签页只占顶部一条(页签那一行),条目列表放在它下面、
    // **不与标签控件重叠** —— 标签控件会绘制自己的页面边框,重叠时会把列表盖住。
    let tabs_x = margin + pane_w + scale(10);
    let tabs_w = (cw - tabs_x - margin).max(scale(200));
    let tabs_h = scale(36);

    // 「只看收藏」开关贴在页签条右侧,单独占一块:页签条是自绘的、只排一行,放不下时
    // 会把多余的页签直接裁掉,所以这个开关**不能**做成页签。
    // 文字宽度按当前字号量;字体还没就绪时量出 0,用下限兜住,免得标题被裁。
    let fav_text_w = ui::text_width(hwnd, app::state().font, FAV_ONLY_LABEL).max(scale(56));
    let fav_h = scale(30);
    let fav_w = (scale(24) + fav_text_w + scale(12)).min(tabs_w / 3);
    let strip_w = (tabs_w - fav_w - scale(10)).max(scale(120));
    place(s.cat_tabs, tabs_x, top, strip_w, tabs_h);
    place(
        s.fav_only_btn,
        tabs_x + tabs_w - fav_w,
        top + (tabs_h - fav_h) / 2,
        fav_w,
        fav_h,
    );

    let list_y = top + tabs_h + scale(4);
    let list_h = (body_h - tabs_h - scale(4)).max(scale(60));
    // 列表占满整行宽度(开关只占页签那一行,不与列表抢位置)。
    place(s.list, tabs_x, list_y, tabs_w, list_h);
    // 空状态提示叠在列表正中:列表此时没有内容,不会互相遮挡。
    place(
        s.empty_hint,
        tabs_x,
        list_y + list_h / 2 - scale(12),
        tabs_w,
        scale(24),
    );
    if app::state().settings.column_widths.len() == MAIN_LIST_COLUMNS.len() {
        for (index, &logical_w) in app::state().settings.column_widths.iter().enumerate() {
            let target_w = scale(logical_w).max(scale(20));
            if ui::listview_get_column_width(s.list, index as i32) != target_w {
                ui::listview_set_column_width(s.list, index as i32, target_w);
            }
        }
    } else {
        let percents: Vec<i32> = MAIN_LIST_COLUMNS.iter().map(|c| c.2).collect();
        set_list_columns(s.list, tabs_w, &percents);
    }

    place(s.status, margin, ch - status_h, cw - margin * 2, status_h);
}

/// 表单形态的排版:一个限宽的竖直列。
fn layout_form(s: &MainUi, x: i32, mut y: i32, w: i32, create: bool) {
    let label_h = scale(20);
    let field_h = scale(30);

    if !create {
        place(s.unlock_title, x, y, w, scale(32));
        y += scale(42);
        place(s.unlock_hint, x, y, w, scale(44));
        y += scale(54);
        place(s.unlock_path_label, x, y, w, label_h);
        y += label_h + scale(4);
        place(s.unlock_path, x, y, w, scale(22));
        y += scale(36);
        place(s.unlock_pw_label, x, y, w, label_h);
        y += label_h + scale(4);
        place(s.unlock_pw, x, y, w, field_h);
        y += field_h + scale(8);
        place(s.unlock_show, x, y, scale(140), scale(24));
        y += scale(42);
        place(s.unlock_btn, x, y, scale(150), scale(36));
        place(s.hello_btn, x + scale(166), y, scale(250), scale(36));
        y += scale(50);
        place(s.forgot_btn, x, y, scale(150), scale(30));
        place(s.goto_create_btn, x + scale(166), y, scale(160), scale(30));
        y += scale(44);
        place(s.unlock_error, x, y, w, scale(48));
    } else {
        place(s.create_title, x, y, w, scale(32));
        y += scale(42);
        place(s.create_hint, x, y, w, scale(44));
        y += scale(54);
        place(s.create_path_label, x, y, w, label_h);
        y += label_h + scale(4);
        place(s.create_path, x, y, w, scale(22));
        y += scale(36);
        place(s.create_pw_label, x, y, w, label_h);
        y += label_h + scale(4);
        place(s.create_pw, x, y, w, field_h);
        y += field_h + scale(8);
        place(s.create_show, x, y, scale(140), scale(24));
        y += scale(32);
        place(s.create_strength, x, y, w, scale(22));
        y += scale(32);
        place(s.create_pw2_label, x, y, w, label_h);
        y += label_h + scale(4);
        place(s.create_pw2, x, y, w, field_h);
        y += field_h + scale(18);
        place(s.create_btn, x, y, scale(180), scale(36));
        y += scale(50);
        place(s.create_error, x, y, w, scale(48));
    }
}

/// 按百分比分配列表列宽,最后一列吃掉剩余宽度。
fn set_list_columns(list: HWND, total: i32, percents: &[i32]) {
    let mut used = 0;
    for (index, percent) in percents.iter().enumerate() {
        let width = if index + 1 == percents.len() {
            (total - used - scale(4)).max(scale(60))
        } else {
            (total * percent / 100).max(scale(60))
        };
        used += width;
        ui::listview_set_column_width(list, index as i32, width);
    }
}

// ---------- 数据刷新 ----------

fn vault_path() -> String {
    crate::paths::vault_path().to_string_lossy().into_owned()
}

fn has_quick_unlock_cache() -> bool {
    let path = vault_path();
    let Ok(header) = VaultService::peek_header(Path::new(&path)) else {
        return false;
    };
    dpapi::load(header.vault_id, header.key_generation).is_some()
}

fn display_title(entry: &Entry) -> String {
    let title = if entry.title.trim().is_empty() {
        "(无标题)".to_string()
    } else {
        entry.title.clone()
    };
    if entry.favorite {
        format!("★ {title}")
    } else {
        title
    }
}

fn display_category(entry: &Entry) -> String {
    if entry.category.trim().is_empty() {
        UNCATEGORIZED.to_string()
    } else {
        entry.category.clone()
    }
}

fn refresh_filters(hwnd: HWND) {
    let s = st(hwnd);

    // 分类 → 横向标签页(全部 / 未分类 / 各分类)
    let previous = ui::tabs_index(s.cat_tabs);
    let categories = app::state().vault.known_categories();
    ui::tabs_clear(s.cat_tabs);
    ui::tabs_add(s.cat_tabs, ALL_CATEGORIES);
    for name in &categories {
        ui::tabs_add(s.cat_tabs, name);
    }
    let last = (TAB_CATEGORY_BASE + categories.len() as i32 - 1).max(TAB_ALL);
    ui::tabs_set_index(s.cat_tabs, previous.clamp(TAB_ALL, last));


    // 标签 → 左侧列表
    let previous_tag = ui::listbox_text(s.tag_list, ui::listbox_index(s.tag_list));
    let tags = app::state().vault.known_tags();
    ui::listbox_clear(s.tag_list);
    ui::listbox_add(s.tag_list, ALL_TAGS);
    for name in &tags {
        ui::listbox_add(s.tag_list, name);
    }
    let tag_index = ui::listbox_find(s.tag_list, &previous_tag);
    ui::send_msg(s.tag_list, LB_SETCURSEL, tag_index.max(0) as usize, 0);
}

fn refresh_list(hwnd: HWND) {
    // 先问"当前选中是哪条":它内部也会 st(hwnd),放到前面,免得与下面的 s 同时活着。
    let selected = selected_entry_id(hwnd);
    let s = st(hwnd);

    // 空状态的提示语要区分原因,先把两份计数取出来(条目引用借到手后就不好再取)。
    let (vault_empty, no_favorites) = {
        let vault = &app::state().vault;
        (vault.entry_count() == 0, !vault.active_entries().any(|e| e.favorite))
    };

    let tab = ui::tabs_index(s.cat_tabs);
    let tag = ui::listbox_text(s.tag_list, ui::listbox_index(s.tag_list));
    let query = ui::get_text(s.search).trim().to_lowercase();
    let sort = ui::combo_index(s.sort_combo);

    // 分类筛选要的名字先取好 —— 条目引用一旦借到手,就不好再调 app::state() 了。
    let category_filter = if tab == TAB_ALL {
        None
    } else {
        app::state()
            .vault
            .known_categories()
            .get((tab - TAB_CATEGORY_BASE) as usize)
            .cloned()
    };

    // 只借引用,不 clone 整个 Entry —— 否则每个条目的密码副本也会跟着复制一遍。
    let mut items: Vec<&Entry> = app::state().vault.active_entries().collect();

    // 分类页签 × 标签 × 搜索词 × 只看收藏,四项「与」关系叠加;
    // 判定规则放在可移植核心 `search` 里,单测覆盖叠加语义。
    let filter = crate::search::ListFilter {
        category: category_filter.as_deref(),
        tag: (!tag.is_empty() && tag != ALL_TAGS).then_some(tag.as_str()),
        query: &query,
        favorites_only: ui::is_checked(s.fav_only_btn),
    };
    items.retain(|e| filter.accept(e));

    // 收藏置顶、搜索命中优先这些规则都在可移植核心 `search` 里(那样才测得到);
    // 界面只负责把下拉框下标翻译成排序方式。
    crate::search::sort_entries(&mut items, crate::search::SortMode::from_combo(sort), &query);

    ui::listview_clear(s.list);
    s.rows.clear();
    for &entry in &items {
        ui::listview_add_row(
            s.list,
            &[
                display_title(entry),
                entry.username.clone(),
                entry.url.clone(),
                display_category(entry),
                entry.tags.join("、"),
                timefmt::local_string(entry.updated),
            ],
        );
        s.rows.push(entry.id.clone());
    }

    if let Some(id) = selected {
        if let Some(index) = s.rows.iter().position(|r| *r == id) {
            ui::listview_select(s.list, index as i32);
        }
    }

    // 空状态:提示要说清"为什么空"。库是空的才引导新建;
    // 勾了「只看收藏」又确实没有收藏,就说明收藏;其余都是筛选没命中。
    let empty = s.rows.is_empty();
    if empty {
        ui::set_text(
            s.empty_hint,
            if vault_empty {
                "还没有条目。点「新建」开始记录。"
            } else if ui::is_checked(s.fav_only_btn) && no_favorites {
                "还没有收藏的条目。"
            } else {
                "没有符合条件的条目。"
            },
        );
    }
    ui::set_visible(s.empty_hint, empty);

    update_status(hwnd);
}

fn refresh_bin(hwnd: HWND) {
    let s = st(hwnd);
    let retention = app::state().settings.bin_retention_days;
    let now = crate::model::now_secs();

    // 同 refresh_list:只借引用,不 clone —— 回收站条目里也有密码副本。
    let mut items: Vec<&Entry> = app::state().vault.deleted_entries().collect();
    items.sort_by(|a, b| b.deleted.cmp(&a.deleted));

    ui::listview_clear(s.bin_list);
    s.bin_rows.clear();
    for &entry in &items {
        let deleted = entry.deleted.unwrap_or(now);
        let remaining = if retention <= 0 {
            "永久保留".to_string()
        } else {
            let days = ((deleted + retention * 86_400 - now) as f64 / 86_400.0).ceil();
            format!("剩余 {} 天", days.max(0.0) as i64)
        };
        ui::listview_add_row(
            s.bin_list,
            &[
                display_title(entry),
                entry.username.clone(),
                display_category(entry),
                timefmt::local_string(deleted),
                remaining,
            ],
        );
        s.bin_rows.push(entry.id.clone());
    }

    ui::set_visible(s.bin_empty_hint, s.bin_rows.is_empty());

    update_status(hwnd);
}

fn update_status(hwnd: HWND) {
    let s = st(hwnd);
    let vault = &app::state().vault;
    let total = vault.entry_count();
    let bin = vault.deleted_count();
    let bin_part = if bin > 0 {
        format!(" · 回收站 {bin} 条")
    } else {
        String::new()
    };
    // 开关打开时才报收藏总数;一条都没有时顺手告诉用户怎么收藏。
    let favorites = vault.active_entries().filter(|e| e.favorite).count();
    let fav_part = if ui::is_checked(s.fav_only_btn) {
        if favorites == 0 {
            " · 还没有收藏：在列表里右键选「收藏」".to_string()
        } else {
            format!(" · 收藏 {favorites} 条")
        }
    } else {
        String::new()
    };
    let path = vault
        .path()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    // 有改动没能落盘:必须一直挂在状态栏上,不能只在出错那一下弹个框。
    // 这些改动**不丢数据**(下次任何一次成功保存都会一并写出),但会随锁定消失,
    // 也不能说成「已保存」——两边都不能骗用户。
    ui::set_text(
        s.status,
        &format!(
            "共 {total} 条，显示 {}{bin_part}{fav_part}{} · {path}",
            s.rows.len(),
            unsaved_note(),
        ),
    );
}

/// 「有改动未落盘」的提示片段;没有就返回空串。
///
/// 单拎出来是因为**每一条**状态栏文字都得带上它 ——「已复制密码」「已收藏」这类
/// 临时反馈是直接 `set_text` 覆盖整行的,顺手就把警告抹掉了,而那正是最不该
/// 消失的时刻(用户刚看到一条「保存失败」,下一手操作就把提示擦掉,等于没提示)。
fn unsaved_note() -> &'static str {
    if app::state().vault.has_unsaved_changes() {
        " · ⚠ 有改动未保存；锁定或退出会丢失"
    } else {
        ""
    }
}

/// 覆盖状态栏显示一条临时反馈,并保留「未落盘」警告。
fn flash_status(hwnd: HWND, message: &str) {
    ui::set_text(st(hwnd).status, &format!("{message}{}", unsaved_note()));
}

/// 有未落盘改动时,先试着补一次保存;成功就当无事发生。
///
/// 返回 `false` 表示用户选择放弃(丢掉这些改动),调用方应当中止当前动作。
fn confirm_discard_unsaved(hwnd: HWND, what: &str) -> bool {
    if !app::state().vault.has_unsaved_changes() {
        return true;
    }
    // 先给一次机会:很多时候失败是暂时的(杀软占住文件、同步冲突刚过去)。
    match app::state().vault.save() {
        Ok(()) => true,
        Err(e) => ui::confirm(
            hwnd,
            &format!(
                "有改动未保存（{e}）。\n\n{what}会丢弃这些改动，确定继续吗？"
            ),
            "改动未保存",
        ),
    }
}

fn selected_entry_id(hwnd: HWND) -> Option<String> {
    let s = st(hwnd);
    let index = ui::listview_selected_index(s.list);
    if index < 0 {
        return None;
    }
    s.rows.get(index as usize).cloned()
}

fn with_entry<R>(hwnd: HWND, id: &str, f: impl FnOnce(&Entry) -> R) -> Option<R> {
    let _ = hwnd;
    app::state()
        .vault
        .document()?
        .entries
        .iter()
        .find(|e| e.id == id)
        .map(f)
}

// ---------- 命令处理 ----------

pub fn on_command(hwnd: HWND, id: usize, code: u16) {
    match id {
        // --- 解锁 ---
        ID_UNLOCK_SHOW if code == BN_CLICKED => {
            let s = st(hwnd);
            let reveal = ui::is_checked(s.unlock_show);
            ui::set_password_char(s.unlock_pw, if reveal { None } else { Some('\u{25CF}') });
        }
        ID_UNLOCK_BTN if code == BN_CLICKED => unlock_with_password(hwnd),
        ID_HELLO_BTN if code == BN_CLICKED => unlock_with_hello(hwnd),
        ID_FORGOT_BTN if code == BN_CLICKED => {
            let path = vault_path();
            if dlg_recovery::show_recover(hwnd, &path) {
                after_unlock(hwnd);
            }
        }
        ID_GOTO_CREATE_BTN if code == BN_CLICKED => {
            app::state().mode = Mode::Create;
            apply_mode(hwnd);
            ui::set_focus(st(hwnd).create_pw);
        }

        // --- 创建 ---
        ID_CREATE_SHOW if code == BN_CLICKED => {
            let s = st(hwnd);
            let reveal = ui::is_checked(s.create_show);
            let ch = if reveal { None } else { Some('\u{25CF}') };
            ui::set_password_char(s.create_pw, ch);
            ui::set_password_char(s.create_pw2, ch);
        }
        ID_CREATE_PW if code == EN_CHANGE => {
            let s = st(hwnd);
            let value = ui::get_secret(s.create_pw);
            let hint = if value.is_empty() {
                "建议至少 12 位，混合大小写字母、数字与符号。".to_string()
            } else {
                let r = strength::evaluate(&value);
                format!("强度：{} · {}", r.label, r.hint)
            };
            ui::set_text(s.create_strength, &hint);
        }
        ID_CREATE_BTN if code == BN_CLICKED => create_vault(hwnd),

        // --- 列表 ---
        ID_NEW_BTN if code == BN_CLICKED => new_entry(hwnd),
        ID_SEARCH if code == EN_CHANGE => refresh_list(hwnd),
        ID_FAV_ONLY_BTN if code == BN_CLICKED => refresh_list(hwnd),
        ID_TAG_LIST if code == LBN_SELCHANGE => refresh_list(hwnd),
        ID_SORT_COMBO if code == CBN_SELCHANGE => refresh_list(hwnd),
        ID_GEN_BTN if code == BN_CLICKED => {
            dlg_generator::show(hwnd, false);
        }
        ID_BIN_BTN if code == BN_CLICKED => {
            app::state().mode = Mode::Bin;
            refresh_bin(hwnd);
            apply_mode(hwnd);
        }
        ID_TAXONOMY_BTN if code == BN_CLICKED => {
            dlg_taxonomy::show(hwnd);
            refresh_filters(hwnd);
            refresh_list(hwnd);
        }
        ID_TRANSFER_BTN if code == BN_CLICKED => {
            // 只有真的改动过库(导入了东西)才刷新,避免一次纯导出也重排列表。
            if dlg_transfer::show(hwnd) {
                refresh_filters(hwnd);
                refresh_list(hwnd);
            }
        }
        ID_SETTINGS_BTN if code == BN_CLICKED => {
            dlg_settings::show(hwnd);
            refresh_filters(hwnd);
            refresh_list(hwnd);
            layout(hwnd);
        }
        ID_LOCK_BTN if code == BN_CLICKED => lock_vault(hwnd, true),

        // --- 回收站 ---
        ID_BIN_RESTORE_BTN if code == BN_CLICKED => {
            let s = st(hwnd);
            let index = ui::listview_selected_index(s.bin_list);
            let id = if index >= 0 {
                s.bin_rows.get(index as usize).cloned()
            } else {
                None
            };
            if let Some(id) = id {
                if let Err(e) = app::state().vault.restore_from_bin(&id) {
                    report_op_failed(hwnd, &e, "恢复失败");
                }
                refresh_filters(hwnd);
                refresh_bin(hwnd);
            }
        }
        ID_BIN_PURGE_BTN if code == BN_CLICKED => {
            let s = st(hwnd);
            let index = ui::listview_selected_index(s.bin_list);
            let id = if index >= 0 {
                s.bin_rows.get(index as usize).cloned()
            } else {
                None
            };
            if let Some(id) = id {
                if ui::confirm(hwnd, "彻底删除后无法恢复，确定继续吗？", "彻底删除") {
                    if let Err(e) = app::state().vault.purge(&id) {
                        report_save_failed(hwnd, &e);
                    }
                    refresh_bin(hwnd);
                }
            }
        }
        ID_BIN_EMPTY_BTN if code == BN_CLICKED => {
            if app::state().vault.deleted_count() == 0 {
                return;
            }
            if ui::confirm(hwnd, "将彻底删除回收站中的所有条目，确定继续吗？", "清空回收站") {
                if let Err(e) = app::state().vault.empty_bin() {
                    report_op_failed(hwnd, &e, "清空失败");
                }
                refresh_bin(hwnd);
            }
        }
        ID_BIN_BACK_BTN if code == BN_CLICKED => {
            app::state().mode = Mode::Unlocked;
            refresh_filters(hwnd);
            refresh_list(hwnd);
            apply_mode(hwnd);
        }
        _ => {}
    }
}

pub fn on_notify(hwnd: HWND, lparam: LPARAM) -> isize {
    let header = unsafe { &*(lparam.0 as *const NMHDR) };
    let code = header.code as i32;
    let list = st(hwnd).list;
    let bin_list = st(hwnd).bin_list;

    // 自定义绘制:增强列表网格线(水平分隔线与垂直列分隔线)
    if code == NM_CUSTOMDRAW && (header.hwndFrom == list || header.hwndFrom == bin_list) {
        use windows::Win32::Foundation::COLORREF;
        use windows::Win32::Graphics::Gdi::{
            CreateSolidBrush, DeleteObject, FillRect, HGDIOBJ,
        };
        use windows::Win32::UI::Controls::{
            CDDS_ITEMPOSTPAINT, CDDS_ITEMPREPAINT, CDDS_PREPAINT, CDRF_DODEFAULT,
            CDRF_NOTIFYITEMDRAW, CDRF_NOTIFYPOSTPAINT, NMLVCUSTOMDRAW,
        };

        let lvcd = unsafe { &*(lparam.0 as *const NMLVCUSTOMDRAW) };
        let stage = lvcd.nmcd.dwDrawStage;

        if stage == CDDS_PREPAINT {
            return CDRF_NOTIFYITEMDRAW as isize;
        }
        if stage == CDDS_ITEMPREPAINT {
            return CDRF_NOTIFYPOSTPAINT as isize;
        }
        if stage == CDDS_ITEMPOSTPAINT {
            let hdc = lvcd.nmcd.hdc;
            let rc = lvcd.nmcd.rc;
            let brush = unsafe { CreateSolidBrush(COLORREF(GRIDLINE_COLOR)) };

            // 1. 下方水平网格线
            let bottom_line = RECT {
                left: rc.left,
                top: rc.bottom - 1,
                right: rc.right,
                bottom: rc.bottom,
            };
            unsafe {
                FillRect(hdc, &bottom_line, brush);
            }

            // 2. 垂直列分隔线
            let col_count = if header.hwndFrom == bin_list {
                BIN_LIST_COLUMNS.len()
            } else {
                MAIN_LIST_COLUMNS.len()
            };
            let mut cur_x = rc.left;
            for col in 0..(col_count - 1) {
                let w = ui::listview_get_column_width(header.hwndFrom, col as i32);
                cur_x += w;
                let col_line = RECT {
                    left: cur_x - 1,
                    top: rc.top,
                    right: cur_x,
                    bottom: rc.bottom,
                };
                unsafe {
                    FillRect(hdc, &col_line, brush);
                }
            }

            unsafe {
                let _ = DeleteObject(HGDIOBJ(brush.0));
            }
            return CDRF_DODEFAULT as isize;
        }
        return CDRF_DODEFAULT as isize;
    }

    if list == header.hwndFrom && code == NM_DBLCLK {
        edit_selected(hwnd);
        return 0;
    }

    let is_list_header = {
        let list_hdr = ui::listview_get_header(list);
        header.hwndFrom == list_hdr
            || (list_hdr.0.is_null() && header.hwndFrom == list)
            || unsafe { windows::Win32::UI::WindowsAndMessaging::GetParent(header.hwndFrom) }.unwrap_or_default() == list
    };

    if is_list_header
        && (code == HDN_ENDTRACKW
            || code == HDN_ENDTRACKA
            || code == HDN_DIVIDERDBLCLICKW
            || code == HDN_DIVIDERDBLCLICKA)
    {
        ui::post_message(hwnd, TSM_COLUMN_RESIZED, 0, 0);
    }

    0
}

/// 用户在主列表中调整了某一列的宽度:读取全部列的物理宽度并转换为 96 DPI 逻辑像素持久化到 data.pkk
pub fn on_column_resized(hwnd: HWND) {
    if app::state().mode != Mode::Unlocked || !app::state().vault.is_unlocked() {
        return;
    }
    let s = st(hwnd);
    let logical_widths: Vec<i32> = (0..MAIN_LIST_COLUMNS.len())
        .map(|i| {
            let phys_w = ui::listview_get_column_width(s.list, i as i32);
            unscale(phys_w).max(20)
        })
        .collect();
    if logical_widths.iter().all(|&w| w > 0) && app::state().settings.column_widths != logical_widths {
        app::state().settings.column_widths = logical_widths.clone();
        let _ = app::state().vault.update_settings(app::state().settings.clone());
    }
}

// ---------- 自绘标签页 ----------

/// 标签条通知:用户点了另一个分类标签,重新筛选列表。
pub fn on_tab_changed(hwnd: HWND) {
    refresh_list(hwnd);
}

// ---------- 右键菜单 ----------

const CMD_COPY_PW: usize = 3001;
const CMD_COPY_USER: usize = 3002;
const CMD_OPEN_URL: usize = 3003;
const CMD_NEW: usize = 3004;
const CMD_EDIT: usize = 3005;
const CMD_DELETE: usize = 3006;
const CMD_FAVORITE: usize = 3007;
/// 用系统默认浏览器打开选中条目的网址。
const CMD_OPEN_BROWSER: usize = 3008;

/// 主列表的右键菜单:复制类操作放最上面。
pub fn on_context_menu(hwnd: HWND) -> bool {
    if app::state().mode != Mode::Unlocked {
        return false;
    }

    let cursor = ui::cursor_pos();
    let list = st(hwnd).list;
    let tabs = st(hwnd).cat_tabs;

    // 1) 分类标签页条上右键 → 分类的增删改
    if ui::point_in_window(tabs, cursor) {
        let hit = ui::tabs_hit_test(tabs, cursor);
        if hit >= TAB_CATEGORY_BASE {
            ui::tabs_set_index(tabs, hit);
            refresh_list(hwnd);
        }
        let is_category = ui::tabs_index(tabs) >= TAB_CATEGORY_BASE;

        let mut menu = ui::PopupMenu::new();
        menu.add(CMD_CAT_NEW, "新建分类…");
        menu.add_item(CMD_CAT_RENAME, "重命名当前分类…", is_category);
        menu.add_item(CMD_CAT_DELETE, "删除当前分类", is_category);
        match menu.track(hwnd) {
            Some(CMD_CAT_NEW) => category_add(hwnd),
            Some(CMD_CAT_RENAME) => category_rename(hwnd),
            Some(CMD_CAT_DELETE) => category_delete(hwnd),
            _ => {}
        }
        return true;
    }

    // 只有右键落在**条目列表**范围内才响应;在工具栏、搜索框、左侧分类栏上
    // 右键一律不弹菜单。
    if !ui::point_in_window(list, cursor) {
        return true;
    }

    let index = ui::listview_item_at(list, cursor);

    let mut menu = ui::PopupMenu::new();

    if index < 0 {
        // 空白处:只在没有针对具体条目时才给「新建」。
        menu.add(CMD_NEW, "新建");
    } else {
        // 条目上:先选中它,菜单里不再出现「新建」。
        ui::listview_select(list, index);

        let (has_password, has_username, has_url, is_favorite) = selected_entry_id(hwnd)
            .and_then(|id| {
                with_entry(hwnd, &id, |e| {
                    (
                        !e.password.is_empty(),
                        !e.username.is_empty(),
                        !e.url.trim().is_empty(),
                        e.favorite,
                    )
                })
            })
            .unwrap_or((false, false, false, false));

        // 没有内容的复制项灰显,点不动。打开网址只按「有没有填」来灰显:
        // 填了但不是 http(s) 的话,点了会给出具体原因,而不是灰着不给解释。
        menu.add_item(CMD_COPY_PW, "复制密码", has_password);
        menu.add_item(CMD_COPY_USER, "复制用户名", has_username);
        menu.add_item(CMD_OPEN_BROWSER, "打开网址", has_url);
        menu.add_item(CMD_OPEN_URL, "复制网址", has_url);
        menu.add_separator();
        menu.add(
            CMD_FAVORITE,
            if is_favorite {
                "取消收藏"
            } else {
                "收藏"
            },
        );
        menu.add_separator();

        // 移动到分类
        let categories = app::state().vault.known_categories();
        let current_category = selected_entry_id(hwnd)
            .and_then(|id| with_entry(hwnd, &id, |e| e.category.clone()))
            .unwrap_or_default();
        let mut sub = menu.submenu("移动到分类");
        for (index, name) in categories.iter().enumerate() {
            if *name == current_category {
                continue;
            }
            sub.add(CMD_MOVE_BASE + index, name);
        }
        if !current_category.is_empty() {
            sub.add(CMD_MOVE_NONE, "未分类");
        }

        menu.add_separator();
        menu.add(CMD_EDIT, "编辑");
        menu.add(CMD_DELETE, "删除");
        // 条目上也给「新建」:列表铺满视口时下方没有空白可点,只靠空白区右键
        // 会让新建彻底没有入口。
        menu.add_separator();
        menu.add(CMD_NEW, "新建");
    }

    match menu.track(hwnd) {
        Some(CMD_COPY_PW) => copy_password(hwnd),
        Some(CMD_COPY_USER) => copy_username(hwnd),
        Some(CMD_OPEN_URL) => copy_selected_url(hwnd),
        Some(CMD_OPEN_BROWSER) => open_selected_url(hwnd),
        Some(CMD_FAVORITE) => toggle_favorite_selected(hwnd),
        Some(CMD_NEW) => new_entry(hwnd),
        Some(CMD_EDIT) => edit_selected(hwnd),
        Some(CMD_DELETE) => delete_selected(hwnd),
        Some(id) if id >= CMD_MOVE_BASE => {
            let name = if id == CMD_MOVE_NONE {
                String::new()
            } else {
                app::state()
                    .vault
                    .known_categories()
                    .get(id - CMD_MOVE_BASE)
                    .cloned()
                    .unwrap_or_default()
            };
            move_selected_to_category(hwnd, name);
        }
        _ => {}
    }
    true
}

// ---------- 分类的增删改 / 移动条目 ----------

const CMD_CAT_NEW: usize = 3010;
const CMD_CAT_RENAME: usize = 3011;
const CMD_CAT_DELETE: usize = 3012;
/// 「移动到分类」子菜单项:4000 起是各分类,CMD_MOVE_NONE 表示「未分类」。
const CMD_MOVE_BASE: usize = 4000;
const CMD_MOVE_NONE: usize = 4999;

fn category_add(hwnd: HWND) {
    let Some(name) = dlg_input::show(hwnd, "新建分类", "请输入分类名称：", "") else {
        return;
    };
    match app::state().vault.add_category(&name) {
        Ok(()) => {
            refresh_filters(hwnd);
            refresh_list(hwnd);
        }
        Err(e) => report_op_failed(hwnd, &e, "新建分类失败"),
    }
}

/// 当前选中的标签页对应的分类名(不是分类页则为空)。
fn current_category(hwnd: HWND) -> Option<String> {
    let tab = ui::tabs_index(st(hwnd).cat_tabs);
    if tab < TAB_CATEGORY_BASE {
        return None;
    }
    app::state()
        .vault
        .known_categories()
        .get((tab - TAB_CATEGORY_BASE) as usize)
        .cloned()
}

fn category_rename(hwnd: HWND) {
    let Some(old) = current_category(hwnd) else {
        ui::info(hwnd, "请先选中一个分类。", "提示");
        return;
    };
    let Some(new) = dlg_input::show(hwnd, "重命名分类", "新的分类名称：", &old) else {
        return;
    };
    match app::state().vault.rename_category(&old, &new) {
        Ok(()) => {
            refresh_filters(hwnd);
            refresh_list(hwnd);
        }
        Err(e) => report_op_failed(hwnd, &e, "重命名失败"),
    }
}

fn category_delete(hwnd: HWND) {
    let Some(name) = current_category(hwnd) else {
        ui::info(hwnd, "请先选中一个分类。", "提示");
        return;
    };

    let affected = app::state()
        .vault
        .active_entries()
        .filter(|e| e.category == name)
        .count();

    let message = if affected == 0 {
        format!("确定删除分类「{name}」吗？")
    } else {
        format!("确定删除分类「{name}」吗？该分类下的 {affected} 个条目会移到「未分类」。")
    };
    if !ui::confirm(hwnd, &message, "删除分类") {
        return;
    }

    match app::state().vault.remove_category(&name) {
        Ok(()) => {
            refresh_filters(hwnd);
            refresh_list(hwnd);
        }
        Err(e) => {
            // 失败时分类其实已从内存里移除,要重绘才能与提示一致。
            report_op_failed(hwnd, &e, "删除失败");
            refresh_filters(hwnd);
            refresh_list(hwnd);
        }
    }
}

fn move_selected_to_category(hwnd: HWND, category: String) {
    let Some(id) = selected_entry_id(hwnd) else {
        return;
    };
    let Some(mut entry) = with_entry(hwnd, &id, |e| e.clone()) else {
        return;
    };
    if entry.category == category {
        return;
    }

    entry.category = category;
    match app::state().vault.update_entry(entry) {
        Ok(()) => {
            refresh_filters(hwnd);
            refresh_list(hwnd);
        }
        Err(e) => {
            // 内存里的分类已经改了,重绘一次才能与提示一致。
            report_op_failed(hwnd, &e, "移动失败");
            refresh_filters(hwnd);
            refresh_list(hwnd);
        }
    }
}

pub fn on_timer(hwnd: HWND, id: usize) {
    match id {
        TIMER_CLIPBOARD => {
            let s = st(hwnd);

            // 先看是否到期。没到期就直接走人 —— 原来这里每一拍都 clone 一次存着的
            // 期望内容(多半就是密码),纯属白白多造一份明文副本。
            let matched = match &s.clipboard_deadline {
                None => {
                    ui::kill_timer(hwnd, TIMER_CLIPBOARD);
                    return;
                }
                Some((deadline, expected)) => {
                    if Instant::now() < *deadline {
                        return;
                    }
                    // 到期了才比对,而且直接借用,不再 clone。
                    clipboard::get_text().as_deref() == Some(expected.as_str())
                }
            };

            if matched {
                clipboard::clear();
            }
            s.clipboard_deadline = None;
            ui::kill_timer(hwnd, TIMER_CLIPBOARD);
            update_status(hwnd);
        }
        TIMER_HELLO => {
            if let Some(verified) = hello::poll() {
                ui::kill_timer(hwnd, TIMER_HELLO);
                finish_hello(hwnd, verified);
            }
        }
        TIMER_IDLE => {
            // 模态对话框(编辑条目、设置等)开着时不锁定:库被锁而对话框仍在,
            // 保存必然失败、用户填的内容白填。对话框关闭后定时器会再评估。
            if app::state().mode != Mode::Unlocked || dialog::is_modal_open() {
                return;
            }
            if let Some(timeout) = idle_timeout_seconds() {
                if idle::idle_seconds() >= timeout {
                    // 空闲锁定不能弹窗打断,未落盘的改动只能靠状态栏提示。
                    lock_vault(hwnd, false);
                }
            }
        }
        TIMER_HELLO_PROBE => {
            // 后台可用性检测出结果了就刷新一次,然后停掉这个短命定时器。
            let Some(available) = hello::availability() else {
                return;
            };
            // 有模态对话框开着就先不动主窗口(apply_mode 会重排布局、还可能抢焦
            // 点),等它关掉再刷新 —— 反正定时器还在跳。
            if dialog::is_modal_open() {
                return;
            }
            ui::kill_timer(hwnd, TIMER_HELLO_PROBE);
            let changed = {
                let s = st(hwnd);
                let changed = s.hello_available != available;
                s.hello_available = available;
                changed
            };
            if changed {
                apply_mode(hwnd);
            }
        }
        _ => {}
    }
}

fn idle_timeout_seconds() -> Option<u64> {
    let minutes = app::state().settings.idle_lock_minutes;
    if minutes < 0 {
        return None;
    }
    if minutes > 0 {
        return Some(minutes as u64 * 60);
    }
    if idle::screen_saver_active() {
        let t = idle::screen_saver_timeout_seconds();
        if t > 0 {
            return Some(t as u64);
        }
    }
    None
}

pub fn on_session_locked(hwnd: HWND) {
    if app::state().mode == Mode::Unlocked {
        // 锁屏是系统事件,不能弹窗 —— 未落盘的改动会随这次锁定丢失。
        lock_vault(hwnd, false);
    }
}

/// 消息循环的按键拦截:回车在这里处理。
///
/// 主窗口是自定义窗口类,不归对话框管理器管 —— `IsDialogMessageW` 拿不到默认
/// 按钮,`BS_DEFPUSHBUTTON` 那套「回车确认」对它就失效了,回车会直接落到获得
/// 焦点的控件上(密码框对回车无反应)。所以在派发前接住,按形态执行确认动作。
pub fn intercept_key(msg: &MSG) -> bool {
    if msg.message != WM_KEYDOWN || msg.wParam.0 as u32 != VK_RETURN {
        return false;
    }
    let hwnd = app::state().main;
    match app::state().mode {
        Mode::Unlock => unlock_with_password(hwnd),
        Mode::Create => create_vault(hwnd),
        _ => return false,
    }
    true
}

// ---------- 解锁 / 创建 / 锁定 ----------

fn set_unlock_error(hwnd: HWND, message: &str) {
    ui::set_text(st(hwnd).unlock_error, message);
}

fn unlock_with_password(hwnd: HWND) {
    let path = vault_path();
    let password = ui::get_secret(st(hwnd).unlock_pw);
    if password.is_empty() {
        set_unlock_error(hwnd, "请输入登录密码。");
        return;
    }

    let s = st(hwnd);
    ui::set_text(s.unlock_error, "正在解锁…");
    ui::set_text(s.unlock_btn, "正在解锁…");
    ui::update_window(hwnd);

    match app::state().vault.open(Path::new(&path), &password) {
        Ok(()) => after_unlock(hwnd),
        Err(e) => {
            let s = st(hwnd);
            ui::set_text(s.unlock_btn, "解锁");
            ui::set_text(s.unlock_error, &e.to_string());
        }
    }
}

fn unlock_with_hello(hwnd: HWND) {
    let path = vault_path();
    let Ok(header) = VaultService::peek_header(Path::new(&path)) else {
        set_unlock_error(hwnd, "无法读取密码本文件。");
        return;
    };
    let Some((dek, _, _)) = dpapi::load(header.vault_id, header.key_generation) else {
        set_unlock_error(hwnd, "免密缓存已失效，请输入登录密码。");
        return;
    };

    // 记下密钥,等验证结果回来再用(验证界面由系统弹出,不阻塞这里)。
    st(hwnd).pending_hello_dek = Some(dek);
    set_unlock_error(hwnd, "等待 Windows Hello 验证…");

    if !hello::request(app::state().main, "验证身份以解锁登录密码") {
        st(hwnd).pending_hello_dek = None;
        set_unlock_error(hwnd, "无法启动 Windows Hello 验证，请输入登录密码。");
        return;
    }

    ui::set_timer(hwnd, TIMER_HELLO, 150);
}

/// 定时器轮询到验证结果后继续收尾。
fn finish_hello(hwnd: HWND, verified: bool) {
    let dek = st(hwnd).pending_hello_dek.take();

    if !verified {
        set_unlock_error(hwnd, "验证未通过，请输入登录密码。");
        return;
    }

    let Some(dek) = dek else {
        return;
    };

    match app::state()
        .vault
        .open_with_cached_key(Path::new(&vault_path()), dek)
    {
        Ok(()) => after_unlock(hwnd),
        Err(e) => {
            dpapi::clear();
            set_unlock_error(hwnd, &e.to_string());
            apply_mode(hwnd);
        }
    }
}

/// 启动时尝试静默免密解锁(仅当缓存不要求 Windows Hello)。
pub fn try_auto_unlock(hwnd: HWND) {
    if app::state().mode != Mode::Unlock {
        return;
    }
    let path = vault_path();
    if !VaultFile::looks_like_vault(Path::new(&path)) {
        return;
    }

    let Ok(header) = VaultService::peek_header(Path::new(&path)) else {
        return;
    };
    let Some((dek, require_hello, locked)) = dpapi::load(header.vault_id, header.key_generation)
    else {
        return;
    };

    // 只有**真的发生过**锁屏或手动锁定(缓存被打上标记)才需要验证。
    // 没锁过屏就直接进入功能界面,不做任何验证。
    if locked {
        if require_hello {
            ui::set_visible(st(hwnd).hello_btn, true);
            unlock_with_hello(hwnd);
        } else {
            dpapi::clear();
        }
        return;
    }

    match app::state().vault.open_with_cached_key(Path::new(&path), dek) {
        Ok(()) => after_unlock(hwnd),
        Err(_) => {
            dpapi::clear();
            apply_mode(hwnd);
        }
    }
}

fn after_unlock(hwnd: HWND) {
    // 设置是加密存在库里的,解锁后同步到内存。
    // 先把值拷出来再写回:不能一边借着 document(它来自 app::state()),
    // 一边在同一句里再取一次 app::state() —— 那是两个可变引用别名。
    let unlocked_settings = app::state().vault.document().map(|d| d.settings.clone());
    if let Some(settings) = unlocked_settings {
        app::state().settings = settings;
    }

    let settings = app::state().settings.clone();

    {
        let vault = &app::state().vault;
        if let Some((id, generation, dek)) = vault.quick_unlock_material() {
            if settings.quick_unlock_enabled {
                // 失败不在这里打扰用户(解锁流程刚走完),但设置界面会如实显示
                // 「免密缓存当前不可用」,不会让人以为它开着。
                let _ = dpapi::store(id, generation, &dek, settings.require_windows_hello);
            } else {
                dpapi::clear();
            }
        }
    }

    let _ = app::state()
        .vault
        .purge_expired_bin_entries(settings.bin_retention_days);

    {
        let state = app::state();
        state.mode = Mode::Unlocked;
    }

    let s = st(hwnd);
    ui::set_text(s.unlock_pw, "");
    ui::set_text(s.unlock_error, "");
    ui::set_text(s.unlock_btn, "解锁");
    ui::set_password_char(s.unlock_pw, Some('\u{25CF}'));
    ui::set_checked(s.unlock_show, false);

    refresh_filters(hwnd);
    refresh_list(hwnd);
    apply_mode(hwnd);

    // 上面几个都会重新 st(hwnd),所以这里重新取,别用上面那个 s。
    ui::set_focus(st(hwnd).search);
}

fn create_vault(hwnd: HWND) {
    let (password, confirm) = {
        let s = st(hwnd);
        (ui::get_secret(s.create_pw), ui::get_secret(s.create_pw2))
    };

    let fail = |msg: &str| ui::set_text(st(hwnd).create_error, msg);

    if password.chars().count() < 6 {
        fail("登录密码太短，请至少使用 6 位字符。");
        return;
    }
    if password != confirm {
        fail("两次输入的登录密码不一致。");
        return;
    }

    let target = crate::paths::vault_path();
    if target.exists() {
        fail("密码本文件已存在，请重启程序后解锁。");
        return;
    }

    ui::set_text(st(hwnd).create_error, "正在创建…");
    ui::update_window(hwnd);

    let recovery_code = match VaultService::create_new(&target, &password) {
        Ok(code) => code,
        Err(e) => {
            fail(&format!("创建失败（{e}）"));
            return;
        }
    };

    if let Err(e) = app::state().vault.open(&target, &password) {
        fail(&format!("创建后打开失败（{e}）"));
        return;
    }

    dlg_recovery::show_code(hwnd, &recovery_code, true);
    after_unlock(hwnd);
}

/// 锁定密码本。
///
/// `may_prompt` 只给**用户主动点的锁定按钮**为 true:空闲超时与锁屏都是系统事件,
/// 那种时刻弹窗既没人在看、又会打断桌面切换,所以那两条路径只能靠状态栏上一直
/// 挂着的提示让用户事先知道「有改动没存上」。
fn lock_vault(hwnd: HWND, may_prompt: bool) {
    if may_prompt && !confirm_discard_unsaved(hwnd, "锁定密码本") {
        return;
    }
    app::state().vault.lock();
    // 要求 Hello 时保留缓存并打标记(下次用指纹进入);否则删掉缓存(下次输密码)。
    dpapi::invalidate_on_lock();
    app::state().mode = Mode::Unlock;

    {
        let s = st(hwnd);
        ui::set_text(s.unlock_pw, "");
        ui::set_text(s.unlock_error, "");
        ui::set_password_char(s.unlock_pw, Some('\u{25CF}'));
        ui::set_checked(s.unlock_show, false);
        s.rows.clear();
        s.bin_rows.clear();
        ui::listview_clear(s.list);
        ui::listview_clear(s.bin_list);
        // 空状态提示不必单独处理:紧接着的 apply_mode 会按新形态统一设置显隐。
        ui::set_text(s.status, "");
    }

    apply_mode(hwnd);
    ui::set_focus(st(hwnd).unlock_pw);
}

// ---------- 条目操作 ----------

/// 「保存失败」要说清改动去哪了。
///
/// 底层是「先改内存再落盘」:失败时改动**仍在内存里**,只是没写进磁盘文件 ——
/// 说成「保存失败」会让用户以为白填了、于是重填一遍,或者以为改动没了。
/// 底层标记(见 `VaultService::dirty`)已经如实记下这件事,这里如实转述。
fn report_save_failed(hwnd: HWND, e: &crate::error::VaultError) {
    report_op_failed(hwnd, e, "保存失败");
}

/// 同 [`report_save_failed`],但保留调用处更贴切的标题(如「恢复失败」「移动失败」)。
///
/// 这些操作同样走「先改内存再落盘」,失败时要给出一致的说明;标题只影响弹窗抬头,
/// 「改动还在内存里」那一段必须每条路径都带上 —— 否则用户会以为这次改动白做了。
fn report_op_failed(hwnd: HWND, e: &crate::error::VaultError, title: &str) {
    ui::error(hwnd, &save_failure_text(e), title);
}

/// 保存失败的完整说明,供主界面弹窗与各对话框共用。
///
/// 只依据 `has_unsaved_changes()` 追加提醒 —— 该标记为真就说明内存里确实压着
/// 没写进文件的内容,与触发它的具体是哪一个操作无关。
pub(crate) fn save_failure_text(e: &crate::error::VaultError) -> String {
    if app::state().vault.has_unsaved_changes() {
        format!(
            "{e}\n\n改动还在内存里，没有写入密码本文件。\
             解决写入问题后随便保存一条即可写入；在那之前不要锁定或退出。"
        )
    } else {
        e.to_string()
    }
}

/// 对话框错误栏空间有限时用的一句话版本(附在主错误信息后面)。
pub(crate) fn save_failure_inline(e: &crate::error::VaultError) -> String {
    if app::state().vault.has_unsaved_changes() {
        format!("{e}（改动在内存里，未写入文件）")
    } else {
        e.to_string()
    }
}

/// 新建条目。工具栏按钮与右键菜单共用这一条路径 ——
/// 之前只有「右键列表下方空白处」一个入口,列表铺满视口后就再也点不到新建了。
fn new_entry(hwnd: HWND) {
    let categories = app::state().vault.known_categories();
    let tags = app::state().vault.known_tags();
    let Some(entry) = dlg_editor::show(hwnd, None, &categories, &tags) else {
        return;
    };
    match app::state().vault.add_entry(entry) {
        Ok(()) => {
            refresh_filters(hwnd);
            refresh_list(hwnd);
        }
        Err(e) => {
            // 条目已在内存里,重绘一次让列表反映出这条未落盘的改动。
            report_save_failed(hwnd, &e);
            refresh_filters(hwnd);
            refresh_list(hwnd);
        }
    }
}

fn edit_selected(hwnd: HWND) {
    let Some(id) = selected_entry_id(hwnd) else {
        return;
    };
    let Some(existing) = with_entry(hwnd, &id, |e| e.clone()) else {
        return;
    };
    let categories = app::state().vault.known_categories();
    let tags = app::state().vault.known_tags();

    if let Some(updated) = dlg_editor::show(hwnd, Some(&existing), &categories, &tags) {
        match app::state().vault.update_entry(updated) {
            Ok(()) => {
                refresh_filters(hwnd);
                refresh_list(hwnd);
            }
            Err(e) => {
                // 改动已在内存里,重绘一次让列表反映出这条未落盘的修改。
                report_save_failed(hwnd, &e);
                refresh_filters(hwnd);
                refresh_list(hwnd);
            }
        }
    }
}

fn delete_selected(hwnd: HWND) {
    let Some(id) = selected_entry_id(hwnd) else {
        return;
    };
    let title = with_entry(hwnd, &id, |e| display_title(e)).unwrap_or_default();
    if !ui::confirm(
        hwnd,
        &format!("确定要把「{title}」移到回收站吗？"),
        "删除",
    ) {
        return;
    }
    if let Err(e) = app::state().vault.move_to_bin(&id) {
        report_save_failed(hwnd, &e);
    }
    refresh_filters(hwnd);
    refresh_list(hwnd);
}

fn toggle_favorite_selected(hwnd: HWND) {
    let Some(id) = selected_entry_id(hwnd) else {
        return;
    };
    match app::state().vault.toggle_favorite(&id) {
        Ok(true) => {
            refresh_list(hwnd);
            flash_status(hwnd, "已收藏，列表中置顶显示");
        }
        Ok(false) => {
            refresh_list(hwnd);
            flash_status(hwnd, "已取消收藏");
        }
        Err(e) => {
            // 收藏状态已在内存里翻转,refresh_list 会重绘星标并同时刷新状态栏警告。
            report_save_failed(hwnd, &e);
            refresh_list(hwnd);
        }
    }
}

fn copy_password(hwnd: HWND) {
    let Some(id) = selected_entry_id(hwnd) else {
        return;
    };
    let Some(password) = with_entry(hwnd, &id, |e| e.password.clone()) else {
        return;
    };
    if password.is_empty() {
        ui::info(hwnd, "这个条目没有填写密码。", "提示");
        return;
    }

    let seconds = app::state().settings.clipboard_clear_seconds;
    clipboard::set_text(&password);

    // 先把剪贴板截止时间写好并结束借用,再写状态栏 —— flash_status 内部会再
    // 调一次 st(hwnd),不能与上面的 &mut 同时活着。
    let message = if seconds > 0 {
        st(hwnd).clipboard_deadline = Some((
            Instant::now() + Duration::from_secs(seconds as u64),
            password,
        ));
        ui::set_timer(hwnd, TIMER_CLIPBOARD, 1000);
        format!("已复制密码，{seconds} 秒后清空剪贴板")
    } else {
        st(hwnd).clipboard_deadline = None;
        "已复制密码".to_string()
    };
    flash_status(hwnd, &message);
}

fn copy_username(hwnd: HWND) {
    let Some(id) = selected_entry_id(hwnd) else {
        return;
    };
    let Some(username) = with_entry(hwnd, &id, |e| e.username.clone()) else {
        return;
    };
    if username.is_empty() {
        ui::info(hwnd, "这个条目没有填写用户名。", "提示");
        return;
    }
    clipboard::set_text(&username);
    flash_status(hwnd, "已复制用户名");
}

/// 复制网址。
///
/// 这里存的往往是 API 服务商的端点地址(base url),而「密码」字段里装的常常是
/// api key —— 所以直接复制比打开浏览器实用得多。
fn copy_selected_url(hwnd: HWND) {
    let Some(id) = selected_entry_id(hwnd) else {
        return;
    };
    let Some(url) = with_entry(hwnd, &id, |e| e.url.clone()) else {
        return;
    };

    let url = url.trim().to_string();
    if url.is_empty() {
        ui::info(hwnd, "这个条目没有填写网址。", "提示");
        return;
    }

    clipboard::set_text(&url);
    flash_status(hwnd, "已复制网址");
}

/// 用系统默认浏览器打开选中条目的网址。
///
/// 网址能不能交给浏览器由可移植核心 `url::normalize_http_url` 决定(白名单只放
/// http/https,没写协议的补 https)。判定失败时如实说明原因 —— 而不是让菜单项
/// 默默灰着,用户看不出为什么点不动。
fn open_selected_url(hwnd: HWND) {
    let Some(id) = selected_entry_id(hwnd) else {
        return;
    };
    let Some(url) = with_entry(hwnd, &id, |e| e.url.clone()) else {
        return;
    };

    if url.trim().is_empty() {
        ui::info(hwnd, "这个条目没有填写网址。", "提示");
        return;
    }

    match ui::open_url_in_browser(&url) {
        Ok(()) => flash_status(hwnd, "已在默认浏览器打开"),
        Err(reason) => ui::warn(hwnd, &reason, "打开网址失败"),
    }
}

pub fn confirm_exit(hwnd: HWND) -> bool {
    // 有改动没能落盘时,退出等于彻底丢失(锁定也一样,但退出后再没有机会补存)。
    if !confirm_discard_unsaved(hwnd, "退出程序") {
        return false;
    }
    // 关闭前把窗口位置/尺寸/最大化状态存进注册表,下次打开直接还原。
    if let Some(placement) = super::window_state::capture(hwnd) {
        super::window_state::save(&placement);
    }
    true
}

/// 窗口销毁时:抹掉内存中的密钥。**不要**清 DPAPI 缓存 ——
/// 那样下次启动就需要重新输入登录密码,而设计目标是「屏保前保持免密」。
pub fn on_destroy() {
    app::state().vault.lock();
}
