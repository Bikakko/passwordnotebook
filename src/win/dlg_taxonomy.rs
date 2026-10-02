//! 分类与标签的管理对话框。
//!
//! 分类和标签是**独立维护**的资产:先在这里建好,写条目时只能从已有项里挑,
//! 避免随手敲出「开发 / 开发2 / dev」这类同义重复。

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use super::app;
use super::{dialog, sys::*, ui};

const CLASS: &str = "PnbDlgTaxonomy";

const ID_HINT: usize = 1;
const ID_CAT_LABEL: usize = 2;
const ID_CAT_INPUT: usize = 3;
const ID_CAT_LIST: usize = 4;
const ID_CAT_ADD: usize = 5;
const ID_CAT_RENAME: usize = 6;
const ID_CAT_DELETE: usize = 7;
const ID_TAG_LABEL: usize = 8;
const ID_TAG_INPUT: usize = 9;
const ID_TAG_LIST: usize = 10;
const ID_TAG_ADD: usize = 11;
const ID_TAG_RENAME: usize = 12;
const ID_TAG_DELETE: usize = 13;
const ID_ERROR: usize = 14;
const ID_CLOSE: usize = 15;

/// 两列共用一套控件句柄布局。
struct Column {
    input: HWND,
    list: HWND,
}

struct TaxonomyState {
    categories: Column,
    tags: Column,
    error: HWND,
}

pub fn show(owner: HWND) {
    let state = Box::new(TaxonomyState {
        categories: Column {
            input: HWND::default(),
            list: HWND::default(),
        },
        tags: Column {
            input: HWND::default(),
            list: HWND::default(),
        },
        error: HWND::default(),
    });
    dialog::open(CLASS, "管理分类与标签", owner, wnd_proc, state, 640, 500);
}

fn st(hwnd: HWND) -> &'static mut TaxonomyState {
    unsafe { ui::state_ref::<TaxonomyState>(hwnd) }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            on_create(hwnd, lparam);
            LRESULT(0)
        }
        WM_COMMAND => {
            on_command(hwnd, (wparam.0 & 0xFFFF) as usize, ((wparam.0 >> 16) & 0xFFFF) as u16);
            LRESULT(0)
        }
        WM_CLOSE => {
            ui::destroy_window(hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn ctl(
    class: &str,
    text: &str,
    style: u32,
    ex: u32,
    parent: HWND,
    id: usize,
    r: (i32, i32, i32, i32),
) -> HWND {
    let handle = ui::create_window(class, text, WS_CHILD | WS_VISIBLE | style, ex, parent, id, 0, 0, 10, 10);
    ui::move_to(
        handle,
        ui::scale(r.0),
        ui::scale(r.1),
        ui::scale(r.2),
        ui::scale(r.3),
    );
    handle
}

fn on_create(hwnd: HWND, lparam: LPARAM) {
    let ptr = unsafe { ui::create_param(lparam) } as *mut TaxonomyState;
    ui::set_user_data(hwnd, ptr as *mut std::ffi::c_void);
    let s = st(hwnd);

    ctl(
        "STATIC",
        "在这里管理分类和标签；条目里只能选已有的项。",
        SS_LEFT,
        0,
        hwnd,
        ID_HINT,
        (20, 14, 590, 40),
    );

    // ---- 分类列 ----
    ctl("STATIC", "分类", SS_LEFT, 0, hwnd, ID_CAT_LABEL, (20, 62, 100, 22));
    s.categories.input = ctl(
        "EDIT",
        "",
        WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL,
        WS_EX_CLIENTEDGE,
        hwnd,
        ID_CAT_INPUT,
        (20, 86, 280, 30),
    );
    s.categories.list = ctl(
        "LISTBOX",
        "",
        WS_BORDER | WS_VSCROLL | WS_TABSTOP | LBS_NOTIFY | LBS_NOINTEGRALHEIGHT,
        0,
        hwnd,
        ID_CAT_LIST,
        (20, 126, 280, 230),
    );
    ctl("BUTTON", "添加", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_CAT_ADD, (20, 366, 86, 34));
    ctl("BUTTON", "重命名", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_CAT_RENAME, (114, 366, 92, 34));
    ctl("BUTTON", "删除", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_CAT_DELETE, (214, 366, 86, 34));

    // ---- 标签列 ----
    ctl("STATIC", "标签", SS_LEFT, 0, hwnd, ID_TAG_LABEL, (330, 62, 100, 22));
    s.tags.input = ctl(
        "EDIT",
        "",
        WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL,
        WS_EX_CLIENTEDGE,
        hwnd,
        ID_TAG_INPUT,
        (330, 86, 280, 30),
    );
    s.tags.list = ctl(
        "LISTBOX",
        "",
        WS_BORDER | WS_VSCROLL | WS_TABSTOP | LBS_NOTIFY | LBS_NOINTEGRALHEIGHT,
        0,
        hwnd,
        ID_TAG_LIST,
        (330, 126, 280, 230),
    );
    ctl("BUTTON", "添加", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_TAG_ADD, (330, 366, 86, 34));
    ctl("BUTTON", "重命名", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_TAG_RENAME, (424, 366, 92, 34));
    ctl("BUTTON", "删除", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_TAG_DELETE, (524, 366, 86, 34));

    s.error = ctl("STATIC", "", SS_LEFT, 0, hwnd, ID_ERROR, (20, 408, 590, 40));
    ctl("BUTTON", "关闭", WS_TABSTOP | BS_DEFPUSHBUTTON, 0, hwnd, ID_CLOSE, (500, 442, 110, 36));

    let font = app::state().font;
    ui::apply_font_to(
        hwnd,
        &[
            ID_HINT, ID_CAT_LABEL, ID_CAT_INPUT, ID_CAT_LIST, ID_CAT_ADD, ID_CAT_RENAME,
            ID_CAT_DELETE, ID_TAG_LABEL, ID_TAG_INPUT, ID_TAG_LIST, ID_TAG_ADD, ID_TAG_RENAME,
            ID_TAG_DELETE, ID_ERROR, ID_CLOSE,
        ],
        font,
    );
    ui::apply_font_to(hwnd, &[ID_CAT_LABEL, ID_TAG_LABEL], app::state().font_bold);

    refresh(hwnd);
    // refresh 会重新 st(hwnd),不能复用上面那把引用。
    ui::set_focus(st(hwnd).categories.input);
}

fn refresh(hwnd: HWND) {
    let s = st(hwnd);

    ui::listbox_clear(s.categories.list);
    for name in app::state().vault.known_categories() {
        ui::listbox_add(s.categories.list, &name);
    }

    ui::listbox_clear(s.tags.list);
    for name in app::state().vault.known_tags() {
        ui::listbox_add(s.tags.list, &name);
    }
}

fn on_command(hwnd: HWND, id: usize, code: u16) {
    let s = st(hwnd);

    // 选中某项时把它填进输入框,方便直接改名。
    if code == LBN_SELCHANGE {
        let (list, input) = match id {
            ID_CAT_LIST => (s.categories.list, s.categories.input),
            ID_TAG_LIST => (s.tags.list, s.tags.input),
            _ => return,
        };
        let index = ui::listbox_index(list);
        if index >= 0 {
            ui::set_text(input, &ui::listbox_text(list, index));
        }
        return;
    }

    if code != BN_CLICKED {
        return;
    }

    let result = match id {
        ID_CAT_ADD => {
            let name = ui::get_text(s.categories.input);
            app::state().vault.add_category(&name)
        }
        ID_CAT_RENAME => {
            let index = ui::listbox_index(s.categories.list);
            if index < 0 {
                ui::set_text(s.error, "请先选中要重命名的分类。");
                return;
            }
            let old = ui::listbox_text(s.categories.list, index);
            let new = ui::get_text(s.categories.input);
            app::state().vault.rename_category(&old, &new)
        }
        ID_CAT_DELETE => {
            let index = ui::listbox_index(s.categories.list);
            if index < 0 {
                ui::set_text(s.error, "请先选中要删除的分类。");
                return;
            }
            let name = ui::listbox_text(s.categories.list, index);
            app::state().vault.remove_category(&name)
        }
        ID_TAG_ADD => {
            let name = ui::get_text(s.tags.input);
            app::state().vault.add_tag(&name)
        }
        ID_TAG_RENAME => {
            let index = ui::listbox_index(s.tags.list);
            if index < 0 {
                ui::set_text(s.error, "请先选中要重命名的标签。");
                return;
            }
            let old = ui::listbox_text(s.tags.list, index);
            let new = ui::get_text(s.tags.input);
            app::state().vault.rename_tag(&old, &new)
        }
        ID_TAG_DELETE => {
            let index = ui::listbox_index(s.tags.list);
            if index < 0 {
                ui::set_text(s.error, "请先选中要删除的标签。");
                return;
            }
            let name = ui::listbox_text(s.tags.list, index);
            app::state().vault.remove_tag(&name)
        }
        ID_CLOSE => {
            ui::destroy_window(hwnd);
            return;
        }
        _ => return,
    };

    match result {
        Ok(()) => {
            ui::set_text(st(hwnd).error, "");
            refresh(hwnd);
        }
        // 失败信息带上「改动已留在内存里」的提醒:这里的增删改同样走「先改内存
        // 再落盘」,写盘失败时改动没丢,只是没进文件 —— 对话框里也要说清楚。
        // 保存失败时内存里其实已经改了,重绘一次让列表与内存一致。
        Err(e) => {
            ui::set_text(st(hwnd).error, &super::main_ui::save_failure_inline(&e));
            refresh(hwnd);
        }
    }
}
