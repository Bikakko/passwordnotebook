//! 「条目详情」对话框:只读展示一条记录的各项信息。
//!
//! 用户名/密码/网址/API 密钥/API 端点五行点一下内容框就复制到剪贴板;
//! 标题只展示;备注留空,由用户自己高亮选取。
//! 打开途径:主列表里双击条目。编辑入口保持独立:右键菜单「编辑」,
//! 或本弹窗底部的「编辑…」。

use std::ffi::c_void;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use zeroize::Zeroizing;

use crate::model::Entry;

use super::main_ui;
use super::{
    app, dialog, dlg_editor,
    sys::*,
    tokens::MASK_CHAR,
    ui::{self, ctl, label},
};

const CLASS: &str = "PnbDlgDetail";

/// 控件 id 刻意避开框架保留的 1(`IDOK`)/2(`IDCANCEL`):`IsDialogMessageW` 会把
/// 回车 / ESC 变成这两个命令送到窗口过程,同号会撞上(dlg_recovery 里踩过同类的
/// IDCANCEL 误判)。
const ID_SHOW: usize = 101;
const ID_STATUS: usize = 102;
const ID_EDIT: usize = 103;
const ID_CLOSE: usize = 104;

/// 行索引 → 两类控件的 id 偏移(标签 / 只读值)。
const ID_LABEL_BASE: usize = 10;
const ID_VALUE_BASE: usize = 20;

/// 七个信息行:(行标签, 复制反馈里的名字, 是否敏感, 是否多行, 是否点击复制)。
const FIELDS: [(&str, &str, bool, bool, bool); 7] = [
    ("标题", "标题", false, false, false),
    ("用户名/账号", "用户名", false, false, true),
    ("密码", "密码", true, false, true),
    ("网址", "网址", false, false, true),
    ("API 密钥", "API 密钥", true, false, true),
    ("API 端点", "API 端点", false, false, true),
    ("备注", "备注", false, true, false),
];

/// 单行只读控件的空值占位(备注不显示占位,留空由用户自己高亮选取)。
/// 空值的遮蔽会临时关掉(见 `refresh_values`),否则「（未填写）」本身
/// 会被点成一片圆点,反而看不出这里是空的。
const EMPTY_HINT: &str = "（未填写）";

// 布局(96 DPI 逻辑像素):标签在左,只读值占满右侧剩余宽度。
const MARGIN: i32 = 20;
const LABEL_W: i32 = 96;
const VALUE_X: i32 = 120;
const VALUE_W: i32 = 460;
const FIELD_H: i32 = 30;
const NOTES_H: i32 = 90;
const BUTTON_H: i32 = 36;
const ROW_Y0: i32 = 20;
const ROW_PITCH: i32 = 38;
const SHOW_Y: i32 = 350;
const STATUS_Y: i32 = 380;
const BUTTONS_Y: i32 = 436;

struct DetailState {
    /// 当前显示的快照;「编辑…」后就地替换为库内的最新值。
    entry: Entry,
    /// 打开本弹窗的主窗口:剪贴板自动清空的计时器挂在它身上。
    main: HWND,
    /// 本次打开期间是否改动过库(供调用方决定要不要刷新列表)。
    changed: bool,
    show_secrets: HWND,
    status: HWND,
    values: [HWND; 7],
}

/// 打开详情弹窗;返回期间是否改动过库。
pub fn show(main: HWND, entry: &Entry) -> bool {
    let state = Box::new(DetailState {
        entry: entry.clone(),
        main,
        changed: false,
        show_secrets: HWND::default(),
        status: HWND::default(),
        values: [HWND::default(); 7],
    });

    // 600 × 520 是**窗口**尺寸(含标题栏与边框):内容底在 472,余下的
    // ≈ 26(标题栏/边框)+ 22(外边距)。漏算标题栏会把底部按钮裁掉 ——
    // 与 0.2.2 修设置窗口越界那次的教训相同。
    let state = dialog::open(CLASS, "条目详情", main, wnd_proc, state, 600, 520);
    state.changed
}

fn st(hwnd: HWND) -> &'static mut DetailState {
    unsafe { ui::state_ref::<DetailState>(hwnd) }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            on_create(hwnd, lparam);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => LRESULT(ui::static_label_reply(wparam.0)),
        WM_COMMAND => {
            let (id, code) = dialog::command_params(wparam);
            on_command(hwnd, id, code);
            LRESULT(0)
        }
        WM_CLOSE => {
            ui::destroy_window(hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// 行索引 → 条目里的对应字段。
fn field_value(entry: &Entry, index: usize) -> &str {
    match index {
        0 => &entry.title,
        1 => &entry.username,
        2 => entry.password.as_str(),
        3 => &entry.url,
        4 => entry.api_key.as_str(),
        5 => &entry.api_endpoint,
        _ => &entry.notes,
    }
}

/// 判空:密码与密钥按原样(与右键菜单的灰显规则一致),其余忽略首尾空白。
fn field_is_empty(entry: &Entry, index: usize) -> bool {
    let value = field_value(entry, index);
    match index {
        2 | 4 => value.is_empty(),
        _ => value.trim().is_empty(),
    }
}

fn on_create(hwnd: HWND, lparam: LPARAM) {
    unsafe { ui::attach_state::<DetailState>(hwnd, lparam) };

    for (index, &(name, _, sensitive, multiline, click_copy)) in FIELDS.iter().enumerate() {
        let y = ROW_Y0 + index as i32 * ROW_PITCH;
        label(hwnd, name, ID_LABEL_BASE + index, (MARGIN, y + 4, LABEL_W, 22));
        let mut style = if multiline {
            WS_BORDER | WS_TABSTOP | WS_VSCROLL | ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL
        } else {
            WS_BORDER | WS_TABSTOP | ES_READONLY | ES_AUTOHSCROLL
        };
        // 遮蔽依靠 EM_SETPASSWORDCHAR(与编辑器同一套开关);带上 ES_PASSWORD,
        // 该消息才会生效。取消遮蔽时把它设回 0,文本就正常显示。
        if sensitive {
            style |= ES_PASSWORD;
        }
        let height = if multiline { NOTES_H } else { FIELD_H };
        let value = ctl(
            "EDIT",
            "",
            style,
            WS_EX_CLIENTEDGE,
            hwnd,
            ID_VALUE_BASE + index,
            (VALUE_X, y, VALUE_W, height),
        );
        // 可点击复制的行挂子类化(连点同一个框也照样复制);标题与备注不挂。
        if click_copy {
            let _ = unsafe { SetWindowSubclass(value, Some(value_proc), index, hwnd.0 as usize) };
        }
        let s = st(hwnd);
        s.values[index] = value;
    }

    // 网址在这里只展示与复制;要打开走主列表右键菜单的「打开网址」。
    //
    // 「显示密码与密钥」水平居中:先按当前字号量出文字宽度、换算回逻辑像素
    // (ctl 会再统一缩放),让控制框贴着文字 —— 复选框的文字在框内是左对齐的,
    // 只是把框居中的话,看得见的文字并不在中间。
    let font = app::state().font;
    let show_text_w = ui::unscale(ui::text_width(hwnd, font, "显示密码与密钥")).max(112);
    let show_w = 24 + show_text_w + 12;
    let show_x = MARGIN + (VALUE_X + VALUE_W - MARGIN - show_w) / 2;
    let s = st(hwnd);
    s.show_secrets = ctl(
        "BUTTON",
        "显示密码与密钥",
        WS_TABSTOP | BS_AUTOCHECKBOX,
        0,
        hwnd,
        ID_SHOW,
        (show_x, SHOW_Y, show_w, 24),
    );
    s.status = label(hwnd, "", ID_STATUS, (MARGIN, STATUS_Y, 560, 44));
    ctl(
        "BUTTON",
        "编辑…",
        WS_TABSTOP | BS_PUSHBUTTON,
        0,
        hwnd,
        ID_EDIT,
        (388, BUTTONS_Y, 96, BUTTON_H),
    );
    let close = ctl(
        "BUTTON",
        "关闭",
        WS_TABSTOP | BS_DEFPUSHBUTTON,
        0,
        hwnd,
        ID_CLOSE,
        (492, BUTTONS_Y, 88, BUTTON_H),
    );

    let mut ids: Vec<usize> = vec![ID_SHOW, ID_STATUS, ID_EDIT, ID_CLOSE];
    for index in 0..FIELDS.len() {
        ids.push(ID_LABEL_BASE + index);
        ids.push(ID_VALUE_BASE + index);
    }
    ui::apply_font_to(hwnd, &ids, font);

    refresh_values(hwnd);
    ui::set_text(st(hwnd).status, "点击文字框即可复制。");
    ui::set_focus(close);
}

/// 把当前快照写进只读控件,并同步遮蔽字符。
///
/// 每次「显示密码与密钥」切换、以及「编辑…」保存成功之后都走这里,
/// 保证界面与 `entry` 快照一致。
fn refresh_values(hwnd: HWND) {
    let s = st(hwnd);
    let reveal = ui::is_checked(s.show_secrets);
    for (index, &(_, _, sensitive, multiline, _)) in FIELDS.iter().enumerate() {
        if field_is_empty(&s.entry, index) {
            // 单行空值显示占位;备注留空,让用户自己高亮选取。空值的遮蔽
            // 一律关掉,否则「（未填写）」本身会被点成一片圆点。
            ui::set_text(s.values[index], if multiline { "" } else { EMPTY_HINT });
            ui::set_password_char(s.values[index], None);
            continue;
        }
        ui::set_text(s.values[index], field_value(&s.entry, index));
        if sensitive {
            ui::set_password_char(s.values[index], if reveal { None } else { Some(MASK_CHAR) });
        }
    }
}

fn on_command(hwnd: HWND, id: usize, code: u16) {
    if code != BN_CLICKED {
        return;
    }
    match id {
        ID_SHOW => refresh_values(hwnd),
        ID_EDIT => edit_entry(hwnd),
        ID_CLOSE => ui::destroy_window(hwnd),
        _ => {}
    }
}

/// 值控件的子类化过程:单击就把该行复制到剪贴板。
///
/// 用子类化而不是 `EN_SETFOCUS`:焦点没变时后者不会再发通知,
/// 「点同一个框两次」就复制不了第二次。
unsafe extern "system" fn value_proc(
    child: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    ref_data: usize,
) -> LRESULT {
    // 敏感行(密码/API 密钥)拦住 Ctrl+C 与右键「复制」:那条路径不经过
    // copy_text,复制出去的内容不会按设置自动清空。要复制请点击本框。
    if msg == WM_COPY && FIELDS.get(id).is_some_and(|field| field.2) {
        return LRESULT(0);
    }
    // 先让控件照常处理(聚焦、放光标),再通知对话框复制;
    // 快速双击的第二下是 WM_LBUTTONDBLCLK,一并当成点击。
    let result = unsafe { DefSubclassProc(child, msg, wparam, lparam) };
    if matches!(msg, WM_LBUTTONDOWN | WM_LBUTTONDBLCLK) {
        copy_clicked(HWND(ref_data as *mut c_void), child);
    }
    result
}

/// 单击某个值控件:复制对应字段。空值、以及不参与点击复制的行(标题、备注)
/// 都无声无息,什么都不做。密码与 API 密钥按敏感内容处理,走主窗口的
/// 自动清空计时。
fn copy_clicked(dialog: HWND, child: HWND) {
    let s = st(dialog);
    let Some(index) = s.values.iter().position(|value| *value == child) else {
        return;
    };
    let (_, what, sensitive, _, click_copy) = FIELDS[index];
    if !click_copy || field_is_empty(&s.entry, index) {
        return;
    }
    // 网址与 API 端点同主列表的待遇(去掉首尾空白),其余按原样。
    let value = field_value(&s.entry, index);
    let text = if matches!(index, 3 | 5) {
        Zeroizing::new(value.trim().to_string())
    } else {
        Zeroizing::new(value.to_string())
    };
    // copy_text 只碰主窗口状态与剪贴板,不跑消息泵、不弹窗,不会重入本弹窗。
    let main = s.main;
    let message = main_ui::copy_text(main, text, what, sensitive);
    ui::set_text(s.status, &message);
}

/// 「编辑…」:嵌套打开条目编辑器,保存后就地刷新本弹窗。
fn edit_entry(hwnd: HWND) {
    let (entry, id) = {
        let s = st(hwnd);
        (s.entry.clone(), s.entry.id.clone())
    };
    let categories = app::state().vault.known_categories();
    let tags = app::state().vault.known_tags();
    let Some(updated) = dlg_editor::show(hwnd, Some(&entry), &categories, &tags) else {
        return;
    };

    let result = app::state().vault.update_entry(updated);

    // 无论落盘成败,内存里都已经是新值(先改内存再落盘);读回来刷新快照,
    // 让显示与库内状态一致 —— 失败时状态行会说明「改动还在内存里」。
    let latest = main_ui::with_entry(hwnd, &id, |e| e.clone());
    {
        let s = st(hwnd);
        if let Some(latest) = latest {
            s.entry = latest;
        }
        s.changed = true;
    }
    refresh_values(hwnd);

    match result {
        Ok(()) => ui::set_text(st(hwnd).status, "已保存修改"),
        Err(e) => {
            let message = format!("保存失败：{}", main_ui::save_failure_inline(&e));
            ui::set_text(st(hwnd).status, &message);
        }
    }
}
