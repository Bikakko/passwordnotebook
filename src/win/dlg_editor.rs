//! 条目编辑对话框。

use std::ffi::c_void;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::model::{now_secs, Entry};
use crate::strength;

use super::app;
use super::{dialog, dlg_generator, sys::*, ui};

const CLASS: &str = "PnbDlgEntryEditor";

const ID_TITLE_LABEL: usize = 1;
const ID_TITLE: usize = 2;
const ID_USER_LABEL: usize = 3;
const ID_USER: usize = 4;
const ID_PW_LABEL: usize = 5;
const ID_PW: usize = 6;
const ID_SHOW: usize = 7;
const ID_GENERATE: usize = 8;
const ID_STRENGTH: usize = 9;
const ID_URL_LABEL: usize = 10;
const ID_URL: usize = 11;
const ID_CAT_LABEL: usize = 12;
const ID_CAT: usize = 13;
const ID_TAGS_LABEL: usize = 14;
const ID_TAGS: usize = 15;
const ID_NOTES_LABEL: usize = 16;
const ID_NOTES: usize = 17;
const ID_ERROR: usize = 18;
const ID_SAVE: usize = 19;
const ID_CANCEL: usize = 20;

/// 分类下拉的第一项,对应「没有分类」。
const UNCATEGORIZED_OPTION: &str = "未分类";

struct EditorState {
    result: Entry,
    categories: Vec<String>,
    available_tags: Vec<String>,
    accepted: bool,
    title: HWND,
    username: HWND,
    password: HWND,
    show_pw: HWND,
    strength: HWND,
    url: HWND,
    category: HWND,
    tags: HWND,
    notes: HWND,
    error: HWND,
}

pub fn show(
    owner: HWND,
    existing: Option<&Entry>,
    categories: &[String],
    tags: &[String],
) -> Option<Entry> {
    let base = match existing {
        Some(e) => e.clone(),
        // id 留空:由 add_entry 统一分配(它在 id 为空时生成,并会把错误捅出去)。
        None => Entry {
            created: now_secs(),
            ..Default::default()
        },
    };

    let state = Box::new(EditorState {
        result: base,
        categories: categories.to_vec(),
        available_tags: tags.to_vec(),
        accepted: false,
        title: HWND::default(),
        username: HWND::default(),
        password: HWND::default(),
        show_pw: HWND::default(),
        strength: HWND::default(),
        url: HWND::default(),
        category: HWND::default(),
        tags: HWND::default(),
        notes: HWND::default(),
        error: HWND::default(),
    });

    let title = if existing.is_some() { "编辑记录" } else { "新建记录" };
    let state = dialog::open(CLASS, title, owner, wnd_proc, state, 580, 780);

    if state.accepted {
        Some(state.result.clone())
    } else {
        None
    }
}

fn st(hwnd: HWND) -> &'static mut EditorState {
    unsafe { ui::state_ref::<EditorState>(hwnd) }
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

fn label(parent: HWND, text: &str, id: usize, r: (i32, i32, i32, i32)) -> HWND {
    ctl("STATIC", text, SS_LEFT, 0, parent, id, r)
}

fn on_create(hwnd: HWND, lparam: LPARAM) {
    let ptr = unsafe { ui::create_param(lparam) } as *mut EditorState;
    ui::set_user_data(hwnd, ptr as *mut c_void);
    let s = st(hwnd);

    label(hwnd, "标题 *", ID_TITLE_LABEL, (20, 14, 200, 22));
    s.title = ctl("EDIT", "", WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, hwnd, ID_TITLE, (20, 36, 540, 30));

    label(hwnd, "用户名 / 账号", ID_USER_LABEL, (20, 76, 200, 22));
    s.username = ctl("EDIT", "", WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, hwnd, ID_USER, (20, 98, 540, 30));

    label(hwnd, "密码", ID_PW_LABEL, (20, 138, 200, 22));
    s.password = ctl("EDIT", "", WS_BORDER | WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, hwnd, ID_PW, (20, 160, 420, 30));
    ctl("BUTTON", "生成", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_GENERATE, (450, 160, 110, 30));
    s.show_pw = ctl("BUTTON", "显示密码", WS_TABSTOP | BS_AUTOCHECKBOX, 0, hwnd, ID_SHOW, (20, 196, 140, 24));
    s.strength = label(hwnd, "", ID_STRENGTH, (20, 224, 540, 22));

    label(hwnd, "网址", ID_URL_LABEL, (20, 254, 200, 22));
    s.url = ctl("EDIT", "", WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, hwnd, ID_URL, (20, 276, 540, 30));

    label(hwnd, "分类", ID_CAT_LABEL, (20, 316, 200, 22));
    s.category = ctl("COMBOBOX", "", WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST, 0, hwnd, ID_CAT, (20, 338, 540, 200));

    label(hwnd, "标签(按住 Ctrl 可多选)", ID_TAGS_LABEL, (20, 374, 300, 22));
    s.tags = ctl(
        "LISTBOX",
        "",
        WS_BORDER | WS_VSCROLL | WS_TABSTOP | LBS_NOTIFY | LBS_EXTENDEDSEL | LBS_NOINTEGRALHEIGHT,
        0,
        hwnd,
        ID_TAGS,
        (20, 396, 540, 120),
    );

    label(hwnd, "备注", ID_NOTES_LABEL, (20, 524, 200, 22));
    s.notes = ctl(
        "EDIT",
        "",
        WS_BORDER | WS_TABSTOP | WS_VSCROLL | ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN,
        WS_EX_CLIENTEDGE,
        hwnd,
        ID_NOTES,
        (20, 546, 540, 110),
    );

    s.error = label(hwnd, "", ID_ERROR, (20, 664, 540, 38));
    ctl("BUTTON", "保存", WS_TABSTOP | BS_DEFPUSHBUTTON, 0, hwnd, ID_SAVE, (20, 706, 130, 38));
    ctl("BUTTON", "取消", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_CANCEL, (162, 706, 100, 38));

    // 填入现有数据。
    let result = s.result.clone();
    ui::set_text(s.title, &result.title);
    ui::set_text(s.username, &result.username);
    ui::set_text(s.password, &result.password);
    ui::set_text(s.url, &result.url);
    ui::set_text(s.notes, &result.notes);

    // 分类:只能从已创建的里面选,第一项是「未分类」。
    let current_category = s.result.category.clone();
    let available_categories = s.categories.clone();
    ui::combo_add(s.category, UNCATEGORIZED_OPTION);
    let mut selected = 0i32;
    for (index, name) in available_categories.iter().enumerate() {
        ui::combo_add(s.category, name);
        if *name == current_category {
            selected = index as i32 + 1;
        }
    }
    ui::combo_set_index(s.category, selected);

    // 标签:同样只能从已创建的里面挑,按住 Ctrl 多选。
    let available_tags = s.available_tags.clone();
    let current_tags = s.result.tags.clone();
    for name in &available_tags {
        ui::listbox_add(s.tags, name);
    }
    for name in &current_tags {
        let index = ui::listbox_find(s.tags, name);
        ui::listbox_set_selected(s.tags, index, true);
    }

    let font = app::state().font;
    ui::apply_font_to(
        hwnd,
        &[
            ID_TITLE_LABEL, ID_TITLE, ID_USER_LABEL, ID_USER, ID_PW_LABEL, ID_PW, ID_SHOW,
            ID_GENERATE, ID_STRENGTH, ID_URL_LABEL, ID_URL, ID_CAT_LABEL, ID_CAT, ID_TAGS_LABEL,
            ID_TAGS, ID_NOTES_LABEL, ID_NOTES, ID_ERROR, ID_SAVE, ID_CANCEL,
        ],
        font,
    );

    // update_strength 会重新 st(hwnd),放到最后调用:上面的 s 到这里已不再使用。
    update_strength(hwnd);
    ui::set_focus(st(hwnd).title);
}

/// 在窗口创建后由调用方补充分类下拉项。
#[allow(dead_code)]
pub fn fill_categories(hwnd: HWND, categories: &[String], current: &str) {
    let s = st(hwnd);
    ui::combo_clear(s.category);
    for c in categories {
        ui::combo_add(s.category, c);
    }
    ui::set_text(s.category, current);
}

fn update_strength(hwnd: HWND) {
    let s = st(hwnd);
    let value = ui::get_secret(s.password);
    let text = if value.is_empty() {
        "尚未填写密码".to_string()
    } else {
        let r = strength::evaluate(&value);
        format!("强度:{} · {}", r.label, r.hint)
    };
    ui::set_text(s.strength, &text);
}

fn on_command(hwnd: HWND, id: usize, code: u16) {
    match id {
        ID_PW if code == EN_CHANGE => update_strength(hwnd),
        ID_SHOW if code == BN_CLICKED => {
            let s = st(hwnd);
            let reveal = ui::is_checked(s.show_pw);
            ui::set_password_char(s.password, if reveal { None } else { Some('\u{25CF}') });
        }
        ID_GENERATE if code == BN_CLICKED => {
            if let Some(password) = dlg_generator::show(hwnd, true) {
                ui::set_text(st(hwnd).password, &password);
                update_strength(hwnd);
            }
        }
        ID_SAVE if code == BN_CLICKED => save(hwnd),
        ID_CANCEL if code == BN_CLICKED => ui::destroy_window(hwnd),
        _ => {}
    }
}

fn save(hwnd: HWND) {
    let s = st(hwnd);
    let title = ui::get_text(s.title).trim().to_string();
    if title.is_empty() {
        ui::set_text(s.error, "请填写标题。");
        return;
    }

    // 分类:下拉里的选择(第 0 项是「未分类」→ 空串)。
    let category_index = ui::combo_index(s.category);
    let category = if category_index <= 0 {
        String::new()
    } else {
        ui::combo_item_text(s.category, category_index)
    };

    // 标签:多选列表里被选中的项。
    let tags: Vec<String> = ui::listbox_selected_indices(s.tags)
        .into_iter()
        .map(|i| ui::listbox_text(s.tags, i))
        .filter(|t| !t.is_empty())
        .collect();

    s.result.title = title;
    s.result.username = ui::get_text(s.username).trim().to_string();
    s.result.password = ui::get_secret(s.password);
    s.result.url = ui::get_text(s.url).trim().to_string();
    s.result.category = category;
    s.result.notes = ui::get_text(s.notes);
    s.result.tags = tags;
    s.result.updated = now_secs();
    s.accepted = true;

    ui::destroy_window(hwnd);
}
