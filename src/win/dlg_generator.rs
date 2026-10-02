//! 密码生成器对话框。

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use zeroize::Zeroizing;

use crate::generator::{self, Options};

use super::app;
use super::clipboard;
use super::dialog;
use super::sys::*;
use super::ui;

const CLASS: &str = "PnbDlgGenerator";

const ID_HINT: usize = 1;
const ID_OUTPUT: usize = 2;
const ID_LENGTH_LABEL: usize = 3;
const ID_LENGTH: usize = 4;
const ID_UPPER: usize = 5;
const ID_LOWER: usize = 6;
const ID_DIGITS: usize = 7;
const ID_SYMBOLS: usize = 8;
const ID_NOAMB: usize = 9;
const ID_REGEN: usize = 10;
const ID_COPY: usize = 11;
const ID_USE: usize = 12;
const ID_CLOSE: usize = 13;
const ID_ERROR: usize = 14;

struct GenState {
    use_button: bool,
    accepted: bool,
    password: Zeroizing<String>,
    length: HWND,
    upper: HWND,
    lower: HWND,
    digits: HWND,
    symbols: HWND,
    no_ambiguous: HWND,
    output: HWND,
    error: HWND,
}

pub fn show(owner: HWND, use_button: bool) -> Option<Zeroizing<String>> {
    let state = Box::new(GenState {
        use_button,
        accepted: false,
        password: Zeroizing::new(String::new()),
        length: HWND::default(),
        upper: HWND::default(),
        lower: HWND::default(),
        digits: HWND::default(),
        symbols: HWND::default(),
        no_ambiguous: HWND::default(),
        output: HWND::default(),
        error: HWND::default(),
    });

    let state = dialog::open(CLASS, "生成密码", owner, wnd_proc, state, 500, 400);
    // 空密码不算成功:生成失败时输出框是空的,别把它当成可用密码交出去。
    if state.accepted && !state.password.is_empty() {
        Some(state.password.clone())
    } else {
        None
    }
}

fn st(hwnd: HWND) -> &'static mut GenState {
    unsafe { ui::state_ref::<GenState>(hwnd) }
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
    let ptr = unsafe { ui::create_param(lparam) } as *mut GenState;
    ui::set_user_data(hwnd, ptr as *mut std::ffi::c_void);

    let s = st(hwnd);
    s.output = ctl(
        "EDIT",
        "",
        WS_BORDER | WS_TABSTOP | ES_READONLY | ES_AUTOHSCROLL,
        WS_EX_CLIENTEDGE,
        hwnd,
        ID_OUTPUT,
        (16, 16, 452, 34),
    );
    ctl("STATIC", "长度", SS_LEFT, 0, hwnd, ID_LENGTH_LABEL, (16, 64, 50, 22));
    s.length = ctl(
        "EDIT",
        "20",
        WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL,
        WS_EX_CLIENTEDGE,
        hwnd,
        ID_LENGTH,
        (70, 62, 70, 26),
    );
    ctl(
        "STATIC",
        "（4 – 256）",
        SS_LEFT,
        0,
        hwnd,
        ID_HINT,
        (152, 64, 100, 22),
    );

    s.upper = ctl("BUTTON", "大写字母（A-Z）", WS_TABSTOP | BS_AUTOCHECKBOX, 0, hwnd, ID_UPPER, (16, 100, 200, 24));
    s.lower = ctl("BUTTON", "小写字母（a-z）", WS_TABSTOP | BS_AUTOCHECKBOX, 0, hwnd, ID_LOWER, (16, 128, 200, 24));
    s.digits = ctl("BUTTON", "数字（0-9）", WS_TABSTOP | BS_AUTOCHECKBOX, 0, hwnd, ID_DIGITS, (16, 156, 200, 24));
    s.symbols = ctl("BUTTON", "符号（!@#$…）", WS_TABSTOP | BS_AUTOCHECKBOX, 0, hwnd, ID_SYMBOLS, (16, 184, 200, 24));
    s.no_ambiguous = ctl(
        "BUTTON",
        "排除易混淆字符（l/1/O/0 等）",
        WS_TABSTOP | BS_AUTOCHECKBOX,
        0,
        hwnd,
        ID_NOAMB,
        (16, 212, 300, 24),
    );

    for (id, checked) in [
        (ID_UPPER, true),
        (ID_LOWER, true),
        (ID_DIGITS, true),
        (ID_SYMBOLS, true),
        (ID_NOAMB, true),
    ] {
        if let Ok(control) = unsafe { windows::Win32::UI::WindowsAndMessaging::GetDlgItem(Some(hwnd), id as i32) } {
            ui::set_checked(control, checked);
        }
    }

    let use_style = if s.use_button {
        BS_DEFPUSHBUTTON
    } else {
        BS_PUSHBUTTON
    };
    ctl("BUTTON", "重新生成", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_REGEN, (16, 316, 120, 34));
    ctl("BUTTON", "复制", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_COPY, (148, 316, 90, 34));
    let use_btn = ctl("BUTTON", "使用此密码", WS_TABSTOP | use_style, 0, hwnd, ID_USE, (250, 316, 130, 34));
    if !s.use_button {
        ui::set_visible(use_btn, false);
    }
    ctl("BUTTON", "关闭", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_CLOSE, (392, 316, 76, 34));
    s.error = ctl("STATIC", "", SS_LEFT, 0, hwnd, ID_ERROR, (16, 286, 452, 24));

    let font = app::state().font;
    ui::apply_font_to(
        hwnd,
        &[
            ID_HINT, ID_OUTPUT, ID_LENGTH_LABEL, ID_LENGTH, ID_UPPER, ID_LOWER, ID_DIGITS,
            ID_SYMBOLS, ID_NOAMB, ID_ERROR, ID_REGEN, ID_COPY, ID_USE, ID_CLOSE,
        ],
        font,
    );

    regenerate(hwnd);
    ui::set_focus(use_btn);
}

fn on_command(hwnd: HWND, id: usize, code: u16) {
    match id {
        ID_REGEN if code == BN_CLICKED => regenerate(hwnd),
        ID_UPPER | ID_LOWER | ID_DIGITS | ID_SYMBOLS | ID_NOAMB if code == BN_CLICKED => {
            regenerate(hwnd)
        }
        ID_LENGTH if code == EN_CHANGE => regenerate(hwnd),
        ID_COPY if code == BN_CLICKED => {
            let password = st(hwnd).password.clone();
            if !password.is_empty() {
                clipboard::set_text(&password);
            }
        }
        ID_USE if code == BN_CLICKED => {
            let s = st(hwnd);
            s.accepted = true;
            ui::destroy_window(hwnd);
        }
        ID_CLOSE if code == BN_CLICKED => ui::destroy_window(hwnd),
        _ => {}
    }
}

fn regenerate(hwnd: HWND) {
    let s = st(hwnd);

    let length: usize = ui::get_text(s.length)
        .trim()
        .parse()
        .unwrap_or(20)
        .clamp(4, 256);

    let options = Options {
        length,
        upper: ui::is_checked(s.upper),
        lower: ui::is_checked(s.lower),
        digits: ui::is_checked(s.digits),
        symbols: ui::is_checked(s.symbols),
        exclude_ambiguous: ui::is_checked(s.no_ambiguous),
    };

    match generator::generate(&options) {
        Ok(password) => {
            s.password = Zeroizing::new(password);
            ui::set_text(s.output, &s.password);
            ui::set_text(s.error, "");
        }
        Err(e) => {
            // 不静默:清掉密码并把原因显示出来,别让用户拿到空密码还不知道为什么。
            s.password = Zeroizing::new(String::new());
            ui::set_text(s.output, "");
            ui::set_text(s.error, &format!("生成失败：{e}"));
        }
    }
}
