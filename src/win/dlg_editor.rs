//! 条目编辑对话框。
//!
//! 布局是左右两部分:左列放主字段(标题/账号|密码/网址/API 密钥/API 端点),
//! 右列放分类/标签/备注,窗口 800×500,小屏(175% 缩放)也能完整放下。
//! 标签是自动换行的复选框,放不下时由 [`TagBoxState`] 负责整区滚动。

use std::ffi::c_void;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Controls::SetScrollInfo;
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetSystemMetrics, SB_VERT, SCROLLINFO, SIF_DISABLENOSCROLL, SIF_PAGE, SIF_POS,
    SIF_RANGE, SM_CXVSCROLL,
};

use crate::model::{now_secs, Entry};

use super::app;
use super::{dialog, dlg_generator, sys::*, tokens::{MASK_CHAR, UNCATEGORIZED}, ui::{self, ctl, label}};

const CLASS: &str = "PnbDlgEntryEditor";
/// 标签复选框容器(自动换行 + 整区滚动)的窗口类。
const TAG_BOX_CLASS: &str = "PnbTagBox";

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
const ID_TAG_BOX: usize = 15;
const ID_NOTES_LABEL: usize = 16;
const ID_NOTES: usize = 17;
const ID_ERROR: usize = 18;
const ID_SAVE: usize = 19;
const ID_CANCEL: usize = 20;
const ID_KEY_LABEL: usize = 21;
const ID_KEY: usize = 22;
const ID_ENDPOINT_LABEL: usize = 23;
const ID_ENDPOINT: usize = 24;
/// 标签复选框的 id 起始值(控件挂在容器上,不参与对话框的 id 分派)。
const ID_TAG_BASE: usize = 300;

/// 布局(96 DPI 逻辑像素,内容区 20..780):
/// 左列放主字段,右列放分类/标签/备注。
const COL_L_W: i32 = 440;
const COL_R_X: i32 = 480;
const COL_R_W: i32 = 300;
/// 左列内部再分两栏(账号|密码、密钥|端点):每栏 214,栏距 12。
const SUB_W: i32 = 214;
const SUB2_X: i32 = 246;

/// 标签复选框区:容器固定在分类下方,内容高了整区滚动。
const TAG_BOX_Y: i32 = 94;
const TAG_BOX_H: i32 = 136;
const TAG_PAD: i32 = 6;
const TAG_ROW_H: i32 = 26;
const TAG_CHECK_H: i32 = 22;
const TAG_HGAP: i32 = 8;
/// 复选框字形的占位(与主界面「只看收藏」的 24 同一口径)与尾部留白。
/// 尾部 8 是刻意留的安全垫:分数 DPI 下 `unscale` 取整 + 容器边框会让真实
/// 客户区比算式少约 1 个逻辑像素,调小它最后一个复选框就会被裁掉一条边。
const TAG_GLYPH_W: i32 = 24;
const TAG_TAIL_W: i32 = 8;

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
    api_key: HWND,
    api_endpoint: HWND,
    category: HWND,
    tag_box: HWND,
    tag_checks: Vec<(String, HWND)>,
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
        api_key: HWND::default(),
        api_endpoint: HWND::default(),
        category: HWND::default(),
        tag_box: HWND::default(),
        tag_checks: Vec::new(),
        notes: HWND::default(),
        error: HWND::default(),
    });

    // 容器类是编辑器自己的,窗口创建前注册(重复注册会失败,无害)。
    let _ = ui::register_class(TAG_BOX_CLASS, tag_box_proc);

    let title = if existing.is_some() { "编辑条目" } else { "新建条目" };
    let state = dialog::open(CLASS, title, owner, wnd_proc, state, 800, 500);

    if state.accepted {
        Some(state.result.clone())
    } else {
        None
    }
}

fn st(hwnd: HWND) -> &'static mut EditorState {
    unsafe { ui::state_ref::<EditorState>(hwnd) }
}

/// 光标停在标签区上时,把滚轮消息转给容器(与系统的悬停滚动开关无关)。
fn forward_wheel_to_tag_box(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    let tag_box = st(hwnd).tag_box;
    // WM_MOUSEWHEEL 的坐标是屏幕坐标。
    let x = (lparam.0 & 0xFFFF) as u16 as i16 as i32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
    let (bx, by, bw, bh) = ui::window_rect(tag_box);
    if (bx..bx + bw).contains(&x) && (by..by + bh).contains(&y) {
        ui::send_msg(tag_box, WM_MOUSEWHEEL, wparam.0, lparam.0);
    }
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
        WM_MOUSEWHEEL => {
            // 滚轮按文档是发给焦点窗口的;系统的「悬停滚动非活动窗口」关掉时,
            // 光标停在标签区上也不会轮到它 —— 补一条与系统设置无关的兜底。
            forward_wheel_to_tag_box(hwnd, wparam, lparam);
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
    unsafe { ui::attach_state::<EditorState>(hwnd, lparam) };
    let font = app::state().font;
    let s = st(hwnd);

    label(hwnd, "标题 *", ID_TITLE_LABEL, (20, 14, 200, 22));
    s.title = ctl("EDIT", "", WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, hwnd, ID_TITLE, (20, 36, COL_L_W, 30));

    // 左列:账号|密码并排;密码列下方右对齐放「生成」,同一行左侧是遮蔽开关。
    label(hwnd, "用户名/账号", ID_USER_LABEL, (20, 76, 200, 22));
    s.username = ctl("EDIT", "", WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, hwnd, ID_USER, (20, 98, SUB_W, 30));
    label(hwnd, "密码", ID_PW_LABEL, (SUB2_X, 76, 120, 22));
    s.password = ctl("EDIT", "", WS_BORDER | WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, hwnd, ID_PW, (SUB2_X, 98, SUB_W, 30));
    ctl("BUTTON", "生成", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_GENERATE, (350, 134, 110, 30));
    s.show_pw = ctl("BUTTON", "显示密码与密钥", WS_TABSTOP | BS_AUTOCHECKBOX, 0, hwnd, ID_SHOW, (20, 137, 200, 24));
    s.strength = label(hwnd, "", ID_STRENGTH, (20, 170, COL_L_W, 22));

    // 网址、密钥、端点各占一整行。
    label(hwnd, "网址", ID_URL_LABEL, (20, 198, 120, 22));
    s.url = ctl("EDIT", "", WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, hwnd, ID_URL, (20, 220, COL_L_W, 30));

    // 密钥按敏感字段处理,与密码共用遮蔽开关。
    label(hwnd, "API 密钥", ID_KEY_LABEL, (20, 256, 200, 22));
    s.api_key = ctl("EDIT", "", WS_BORDER | WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, hwnd, ID_KEY, (20, 278, COL_L_W, 30));
    label(hwnd, "API 端点", ID_ENDPOINT_LABEL, (20, 314, 120, 22));
    s.api_endpoint = ctl("EDIT", "", WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, hwnd, ID_ENDPOINT, (20, 336, COL_L_W, 30));

    // 遮蔽字符统一成 ●(ES_PASSWORD 的系统默认是 *),与详情弹窗一致。
    ui::set_password_char(s.password, Some(MASK_CHAR));
    ui::set_password_char(s.api_key, Some(MASK_CHAR));

    // 右列:分类 / 标签(自动换行的复选框,可滚动)/ 备注;与左边标题同一行起排。
    label(hwnd, "分类", ID_CAT_LABEL, (COL_R_X, 14, 120, 22));
    s.category = ctl("COMBOBOX", "", WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST, 0, hwnd, ID_CAT, (COL_R_X, 36, COL_R_W, 200));

    label(hwnd, "标签", ID_TAGS_LABEL, (COL_R_X, 72, 120, 22));
    // 容器自己不占 Tab 停靠点(WS_EX_CONTROLPARENT 让 Tab 直接进入复选框);
    // WS_CLIPCHILDREN 防止滚动挪动子控件时父子互相擦画导致闪烁。
    s.tag_box = ctl(
        TAG_BOX_CLASS,
        "",
        WS_VSCROLL | WS_BORDER | WS_CLIPCHILDREN,
        WS_EX_CLIENTEDGE | WS_EX_CONTROLPARENT,
        hwnd,
        ID_TAG_BOX,
        (COL_R_X, TAG_BOX_Y, COL_R_W, TAG_BOX_H),
    );

    label(hwnd, "备注", ID_NOTES_LABEL, (COL_R_X, 236, 120, 22));
    s.notes = ctl(
        "EDIT",
        "",
        WS_BORDER | WS_TABSTOP | WS_VSCROLL | ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN,
        WS_EX_CLIENTEDGE,
        hwnd,
        ID_NOTES,
        (COL_R_X, 258, COL_R_W, 110),
    );

    s.error = label(hwnd, "", ID_ERROR, (20, 378, 760, 38));
    ctl("BUTTON", "保存", WS_TABSTOP | BS_DEFPUSHBUTTON, 0, hwnd, ID_SAVE, (550, 420, 130, 36));
    ctl("BUTTON", "取消", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_CANCEL, (680, 420, 100, 36));

    // 填入现有数据。
    let result = s.result.clone();
    ui::set_text(s.title, &result.title);
    ui::set_text(s.username, &result.username);
    ui::set_text(s.password, &result.password);
    ui::set_text(s.url, &result.url);
    ui::set_text(s.api_key, &result.api_key);
    ui::set_text(s.api_endpoint, &result.api_endpoint);
    ui::set_text(s.notes, &result.notes);

    // 分类:只能从已创建的里面选,第一项是「未分类」。
    let current_category = s.result.category.clone();
    let available_categories = s.categories.clone();
    ui::combo_add(s.category, UNCATEGORIZED);
    let mut selected = 0i32;
    for (index, name) in available_categories.iter().enumerate() {
        ui::combo_add(s.category, name);
        if *name == current_category {
            selected = index as i32 + 1;
        }
    }
    ui::combo_set_index(s.category, selected);

    // 标签:一排自动换行的复选框,勾选状态来自条目。
    {
        let available_tags = s.available_tags.clone();
        let current_tags = s.result.tags.clone();
        // 容器始终带 WS_VSCROLL,布局时先把滚动条宽度让出来。
        let usable = COL_R_W - TAG_PAD * 2 - tag_scrollbar_width();
        let mut x = TAG_PAD;
        let mut y = TAG_PAD;
        let mut checks: Vec<(String, HWND)> = Vec::new();
        let mut placed: Vec<(HWND, i32, i32, i32)> = Vec::new();
        for (index, name) in available_tags.iter().enumerate() {
            let text_w = ui::unscale(ui::text_width(hwnd, font, name)).max(24);
            let box_w = TAG_GLYPH_W + text_w + TAG_TAIL_W;
            if x > TAG_PAD && x + box_w > TAG_PAD + usable {
                x = TAG_PAD;
                y += TAG_ROW_H;
            }
            let check = ctl(
                "BUTTON",
                name,
                WS_TABSTOP | BS_AUTOCHECKBOX,
                0,
                s.tag_box,
                ID_TAG_BASE + index,
                (x, y, box_w, TAG_CHECK_H),
            );
            if !font.is_invalid() {
                ui::send_msg(check, WM_SETFONT, font.0 as usize, 1);
            }
            ui::set_checked(check, current_tags.iter().any(|t| t == name));
            checks.push((name.clone(), check));
            placed.push((check, x, y, box_w));
            x += box_w + TAG_HGAP;
        }
        let content_h = y + TAG_CHECK_H + TAG_PAD;
        s.tag_checks = checks;
        tag_box_init(s.tag_box, content_h, placed);
    }

    ui::apply_font_to(
        hwnd,
        &[
            ID_TITLE_LABEL, ID_TITLE, ID_USER_LABEL, ID_USER, ID_PW_LABEL, ID_PW, ID_SHOW,
            ID_GENERATE, ID_STRENGTH, ID_URL_LABEL, ID_URL, ID_KEY_LABEL, ID_KEY,
            ID_ENDPOINT_LABEL, ID_ENDPOINT, ID_CAT_LABEL, ID_CAT, ID_TAGS_LABEL, ID_NOTES_LABEL,
            ID_NOTES, ID_ERROR, ID_SAVE, ID_CANCEL,
        ],
        font,
    );

    // update_strength 会重新 st(hwnd),放到最后调用:上面的 s 到这里已不再使用。
    update_strength(hwnd);
    ui::set_focus(st(hwnd).title);
}

// ---------- 标签复选框容器 ----------

/// 容器状态:记住每个复选框的原始位置,滚动时整体上下移动。
struct TagBoxState {
    /// 复选框:句柄 + 未滚动时的位置与尺寸(物理像素,相对容器)。
    children: Vec<(HWND, i32, i32, i32, i32)>,
    /// 当前滚动位置(物理像素,0 = 顶部)。
    scroll: i32,
    /// 内容总高与视口高(物理像素)。
    content_h: i32,
    view_h: i32,
    /// 一次滚动的步长(一行的高度,物理像素)。
    line_h: i32,
    /// 高精度滚轮(触控板)的小步累积:满一格(120)才滚动。
    wheel_accum: i32,
}

fn tag_box_state(hwnd: HWND) -> Option<&'static mut TagBoxState> {
    let ptr = unsafe { ui::user_data::<TagBoxState>(hwnd) };
    (!ptr.is_null()).then(|| unsafe { &mut *ptr })
}

/// 垂直滚动条占的宽度(逻辑像素),布局复选框时先让出来。
fn tag_scrollbar_width() -> i32 {
    let physical = unsafe { GetSystemMetrics(SM_CXVSCROLL) };
    ui::unscale(physical).max(12)
}

/// 子控件摆好之后挂上状态并设置滚动条;`content_h`/坐标都是逻辑像素。
fn tag_box_init(hwnd: HWND, content_h: i32, children: Vec<(HWND, i32, i32, i32)>) {
    let view_h = ui::client_size(hwnd).1;
    let state = Box::new(TagBoxState {
        children: children
            .into_iter()
            .map(|(child, x, y, w)| {
                (
                    child,
                    ui::scale(x),
                    ui::scale(y),
                    ui::scale(w),
                    ui::scale(TAG_CHECK_H),
                )
            })
            .collect(),
        scroll: 0,
        content_h: ui::scale(content_h),
        view_h,
        line_h: ui::scale(TAG_ROW_H).max(1),
        wheel_accum: 0,
    });
    ui::set_user_data(hwnd, Box::into_raw(state) as *mut c_void);
    tag_box_update_scrollbar(hwnd);
}

/// 把滚动范围 / 页大小 / 当前位置同步给滚动条。
fn tag_box_update_scrollbar(hwnd: HWND) {
    let Some(s) = tag_box_state(hwnd) else {
        return;
    };
    let info = SCROLLINFO {
        cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
        // SIF_DISABLENOSCROLL:装得下时禁用滚动条而不是把它整个移除,右侧宽度稳定。
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS | SIF_DISABLENOSCROLL,
        nMin: 0,
        nMax: (s.content_h - 1).max(0),
        nPage: s.view_h.max(1) as u32,
        nPos: s.scroll,
        ..Default::default()
    };
    unsafe {
        let _ = SetScrollInfo(hwnd, SB_VERT, &info, true);
    }
}

/// 滚动 `delta` 像素(正数向下);内容装得下时什么也不做。
fn tag_box_scroll(hwnd: HWND, delta: i32) {
    let Some(s) = tag_box_state(hwnd) else {
        return;
    };
    let max_scroll = (s.content_h - s.view_h).max(0);
    let moved = (s.scroll + delta).clamp(0, max_scroll) - s.scroll;
    if moved == 0 {
        return;
    }
    s.scroll += moved;
    for (child, x, y, w, h) in &s.children {
        ui::move_to(*child, *x, *y - s.scroll, *w, *h);
    }
    tag_box_update_scrollbar(hwnd);
}

fn tag_box_on_vscroll(hwnd: HWND, wparam: WPARAM) {
    let Some(s) = tag_box_state(hwnd) else {
        return;
    };
    let code = wparam.0 & 0xFFFF;
    let (page, line) = (s.view_h, s.line_h);
    let target = match code {
        SB_LINEUP => s.scroll - line,
        SB_LINEDOWN => s.scroll + line,
        SB_PAGEUP => s.scroll - page,
        SB_PAGEDOWN => s.scroll + page,
        // 拖动滑块:位置在 16 位高位字里,标签多到 content_h > 65535 物理像素
        // 才会失真(实际到不了),钳位兜底。
        SB_THUMBTRACK | SB_THUMBPOSITION => ((wparam.0 >> 16) & 0xFFFF) as i32,
        _ => return,
    };
    tag_box_scroll(hwnd, target - s.scroll);
}

fn tag_box_on_wheel(hwnd: HWND, wparam: WPARAM) {
    let Some(s) = tag_box_state(hwnd) else {
        return;
    };
    // 滚轮高位字是有符号的:正数 = 远离用户 = 向上滚。触控板会给 ±40 这类
    // 小步,先累积到一整格(WHEEL_DELTA = 120)再滚三行,免得一点就跳。
    let delta = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
    s.wheel_accum += delta;
    let notches = s.wheel_accum / 120;
    if notches == 0 {
        return;
    }
    s.wheel_accum -= notches * 120;
    let amount = -notches * (s.line_h * 3).max(1);
    tag_box_scroll(hwnd, amount);
}

/// 某个复选框获得键盘焦点时(Tab 进来),把滚动区挪到能看见它的位置。
fn tag_box_ensure_visible(hwnd: HWND, child: HWND) {
    let Some(s) = tag_box_state(hwnd) else {
        return;
    };
    let Some((_, _, y, _, h)) = s.children.iter().find(|(c, ..)| *c == child) else {
        return;
    };
    let pad = ui::scale(4);
    let top = *y - s.scroll;
    let bottom = top + *h;
    let delta = if top < pad {
        top - pad
    } else if bottom > s.view_h - pad {
        bottom - (s.view_h - pad)
    } else {
        return;
    };
    tag_box_scroll(hwnd, delta);
}

/// 容器窗口过程:只管滚动;布局与勾选状态由编辑器在创建时摆好。
unsafe extern "system" fn tag_box_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_VSCROLL => {
            tag_box_on_vscroll(hwnd, wparam);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            tag_box_on_wheel(hwnd, wparam);
            LRESULT(0)
        }
        WM_COMMAND => {
            // 子复选框拿到键盘焦点(Tab 进来):滚到能看见它的位置。
            let code = ((wparam.0 >> 16) & 0xFFFF) as u16;
            if code == BN_SETFOCUS {
                tag_box_ensure_visible(hwnd, HWND(lparam.0 as *mut c_void));
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_NCDESTROY => {
            // 窗口销毁时回收状态;复选框是子窗口,会随容器一起销毁。
            let ptr = unsafe { ui::user_data::<TagBoxState>(hwnd) };
            if !ptr.is_null() {
                drop(unsafe { Box::from_raw(ptr) });
                ui::set_user_data(hwnd, std::ptr::null_mut());
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn update_strength(hwnd: HWND) {
    let s = st(hwnd);
    let value = ui::get_secret(s.password);
    // 还没填密码时不显示任何强度说明(空行比「尚未填写密码」这种废话干净)。
    let text = super::main_ui::strength_line(&value, "");
    ui::set_text(s.strength, &text);
}

fn on_command(hwnd: HWND, id: usize, code: u16) {
    match id {
        ID_PW if code == EN_CHANGE => update_strength(hwnd),
        ID_SHOW if code == BN_CLICKED => {
            let s = st(hwnd);
            let reveal = ui::is_checked(s.show_pw);
            let ch = if reveal { None } else { Some(MASK_CHAR) };
            ui::set_password_char(s.password, ch);
            ui::set_password_char(s.api_key, ch);
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

    // 标签:被勾选的复选框。
    let tags: Vec<String> = s
        .tag_checks
        .iter()
        .filter(|(_, check)| ui::is_checked(*check))
        .map(|(name, _)| name.clone())
        .collect();

    s.result.title = title;
    s.result.username = ui::get_text(s.username).trim().to_string();
    s.result.password = ui::get_secret(s.password);
    s.result.url = ui::get_text(s.url).trim().to_string();
    s.result.api_key = ui::get_secret(s.api_key);
    s.result.api_endpoint = ui::get_text(s.api_endpoint).trim().to_string();
    s.result.category = category;
    s.result.notes = ui::get_text(s.notes);
    s.result.tags = tags;
    // 收藏只从右键菜单改,编辑框不动它 —— `result` 是从原条目克隆来的,
    // 这里不碰 `favorite` 就等于原样保留。
    s.result.updated = now_secs();
    s.accepted = true;

    ui::destroy_window(hwnd);
}
