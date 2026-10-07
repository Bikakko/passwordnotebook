//! 分类与标签的管理对话框。
//!
//! 分类和标签是**独立维护**的资产:先在这里建好,写条目时只能从已有项里挑,
//! 避免随手敲出「开发 / 开发2 / dev」这类同义重复。

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use super::app;
use super::{dialog, sys::*, ui::{self, ctl}};

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

/// 两列的控件 id 组(建列与命令分派共用)。
#[derive(Clone, Copy)]
struct ColumnIds {
    label: usize,
    input: usize,
    list: usize,
    add: usize,
    rename: usize,
    delete: usize,
}

const CATEGORY_IDS: ColumnIds = ColumnIds {
    label: ID_CAT_LABEL,
    input: ID_CAT_INPUT,
    list: ID_CAT_LIST,
    add: ID_CAT_ADD,
    rename: ID_CAT_RENAME,
    delete: ID_CAT_DELETE,
};

const TAG_IDS: ColumnIds = ColumnIds {
    label: ID_TAG_LABEL,
    input: ID_TAG_INPUT,
    list: ID_TAG_LIST,
    add: ID_TAG_ADD,
    rename: ID_TAG_RENAME,
    delete: ID_TAG_DELETE,
};

/// 列的操作目标:分类或标签。
#[derive(Clone, Copy)]
enum Target {
    Categories,
    Tags,
}

impl Target {
    /// 列名(错误提示用)。
    fn noun(self) -> &'static str {
        match self {
            Target::Categories => "分类",
            Target::Tags => "标签",
        }
    }

    /// 该列在状态里的句柄组。
    fn column(self, s: &TaxonomyState) -> &Column {
        match self {
            Target::Categories => &s.categories,
            Target::Tags => &s.tags,
        }
    }
}

/// 列上的按钮动作。
#[derive(Clone, Copy)]
enum Action {
    Add,
    Rename,
    Delete,
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
    dialog::open(CLASS, "管理分类与标签", owner, wnd_proc, state, 640, 540);
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

fn on_create(hwnd: HWND, lparam: LPARAM) {
    unsafe { ui::attach_state::<TaxonomyState>(hwnd, lparam) };
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

    // ---- 分类列 / 标签列(同一套布局,整体右移一列宽)----
    s.categories = create_column(hwnd, 20, "分类", CATEGORY_IDS);
    s.tags = create_column(hwnd, 330, "标签", TAG_IDS);

    s.error = ctl("STATIC", "", SS_LEFT, 0, hwnd, ID_ERROR, (20, 410, 460, 40));
    ctl("BUTTON", "关闭", WS_TABSTOP | BS_DEFPUSHBUTTON, 0, hwnd, ID_CLOSE, (510, 462, 110, 36));

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

    fill_column(&s.categories, app::state().vault.known_categories());
    fill_column(&s.tags, app::state().vault.known_tags());
}

/// 建一列:标题 + 输入框 + 列表 + 添加/重命名/删除按钮;`x` 为列左缘(96 DPI)。
fn create_column(hwnd: HWND, x: i32, title: &str, ids: ColumnIds) -> Column {
    ctl("STATIC", title, SS_LEFT, 0, hwnd, ids.label, (x, 62, 100, 22));
    let input = ctl(
        "EDIT",
        "",
        WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL,
        WS_EX_CLIENTEDGE,
        hwnd,
        ids.input,
        (x, 86, 280, 30),
    );
    let list = ctl(
        "LISTBOX",
        "",
        WS_BORDER | WS_VSCROLL | WS_TABSTOP | LBS_NOTIFY | LBS_NOINTEGRALHEIGHT,
        0,
        hwnd,
        ids.list,
        (x, 126, 280, 230),
    );
    ctl("BUTTON", "添加", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ids.add, (x, 366, 86, 36));
    ctl("BUTTON", "重命名", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ids.rename, (x + 94, 366, 92, 36));
    ctl("BUTTON", "删除", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ids.delete, (x + 194, 366, 86, 36));
    Column { input, list }
}

/// 按钮 id → (目标列, 动作);非操作按钮返回 `None`。
fn split_id(id: usize) -> Option<(Target, Action)> {
    match id {
        ID_CAT_ADD => Some((Target::Categories, Action::Add)),
        ID_CAT_RENAME => Some((Target::Categories, Action::Rename)),
        ID_CAT_DELETE => Some((Target::Categories, Action::Delete)),
        ID_TAG_ADD => Some((Target::Tags, Action::Add)),
        ID_TAG_RENAME => Some((Target::Tags, Action::Rename)),
        ID_TAG_DELETE => Some((Target::Tags, Action::Delete)),
        _ => None,
    }
}

/// 列表 id → 所属列(选中联动输入框用)。
fn list_target(id: usize) -> Option<Target> {
    match id {
        ID_CAT_LIST => Some(Target::Categories),
        ID_TAG_LIST => Some(Target::Tags),
        _ => None,
    }
}

/// 当前列表选中项;未选中时给出「请先选中要…的…」提示并返回 `None`。
fn selected_name(s: &TaxonomyState, target: Target, verb: &str) -> Option<String> {
    let column = target.column(s);
    let index = ui::listbox_index(column.list);
    if index < 0 {
        ui::set_text(s.error, &format!("请先选中要{verb}的{}。", target.noun()));
        return None;
    }
    Some(ui::listbox_text(column.list, index))
}

/// 重建一列的列表内容。
fn fill_column(column: &Column, names: Vec<String>) {
    ui::listbox_clear(column.list);
    for name in &names {
        ui::listbox_add(column.list, name);
    }
}

fn on_command(hwnd: HWND, id: usize, code: u16) {
    let s = st(hwnd);

    // 选中某项时把它填进输入框,方便直接改名。
    if code == LBN_SELCHANGE {
        let Some(target) = list_target(id) else {
            return;
        };
        let column = target.column(s);
        let index = ui::listbox_index(column.list);
        if index >= 0 {
            ui::set_text(column.input, &ui::listbox_text(column.list, index));
        }
        return;
    }

    if code != BN_CLICKED {
        return;
    }

    let Some((target, action)) = split_id(id) else {
        if id == ID_CLOSE {
            ui::destroy_window(hwnd);
        }
        return;
    };

    let column = target.column(s);
    let name = ui::get_text(column.input);

    let result = match (action, target) {
        (Action::Add, Target::Categories) => app::state().vault.add_category(&name),
        (Action::Add, Target::Tags) => app::state().vault.add_tag(&name),
        (Action::Rename, Target::Categories) => {
            let Some(old) = selected_name(s, target, "重命名") else {
                return;
            };
            app::state().vault.rename_category(&old, &name)
        }
        (Action::Rename, Target::Tags) => {
            let Some(old) = selected_name(s, target, "重命名") else {
                return;
            };
            app::state().vault.rename_tag(&old, &name)
        }
        (Action::Delete, Target::Categories) => {
            let Some(old) = selected_name(s, target, "删除") else {
                return;
            };
            app::state().vault.remove_category(&old)
        }
        (Action::Delete, Target::Tags) => {
            let Some(old) = selected_name(s, target, "删除") else {
                return;
            };
            app::state().vault.remove_tag(&old)
        }
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
