//! 单行文本输入对话框(新建/重命名分类或标签用)。

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use super::app;
use super::{dialog, sys::*, ui::{self, ctl}};

const CLASS: &str = "PnbDlgInput";

const ID_PROMPT: usize = 1;
const ID_EDIT: usize = 2;
const ID_OK: usize = 3;
const ID_CANCEL: usize = 4;

struct InputState {
    prompt: String,
    edit: HWND,
    accepted: bool,
    value: String,
}

/// 弹出一个单行输入框;确定返回 Some(文本),取消返回 None。
pub fn show(owner: HWND, title: &str, prompt: &str, initial: &str) -> Option<String> {
    let state = Box::new(InputState {
        prompt: prompt.to_string(),
        edit: HWND::default(),
        accepted: false,
        value: initial.to_string(),
    });

    let state = dialog::open(CLASS, title, owner, wnd_proc, state, 460, 210);
    if state.accepted {
        Some(state.value.clone())
    } else {
        None
    }
}

fn st(hwnd: HWND) -> &'static mut InputState {
    unsafe { ui::state_ref::<InputState>(hwnd) }
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
    unsafe { ui::attach_state::<InputState>(hwnd, lparam) };
    let s = st(hwnd);

    // 提示文案由调用方通过窗口标题之外的方式给出,这里直接用标题即可;
    // 为了信息完整再放一行说明。
    let prompt = s.prompt.clone();
    ctl("STATIC", &prompt, SS_LEFT, 0, hwnd, ID_PROMPT, (20, 16, 420, 24));

    let initial = s.value.clone();
    s.edit = ctl(
        "EDIT",
        &initial,
        WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL,
        WS_EX_CLIENTEDGE,
        hwnd,
        ID_EDIT,
        (20, 46, 420, 30),
    );
    ctl("BUTTON", "确定", WS_TABSTOP | BS_DEFPUSHBUTTON, 0, hwnd, ID_OK, (222, 96, 100, 36));
    ctl("BUTTON", "取消", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_CANCEL, (330, 96, 110, 36));

    let font = app::state().font;
    ui::apply_font_to(hwnd, &[ID_PROMPT, ID_EDIT, ID_OK, ID_CANCEL], font);

    ui::set_focus(s.edit);
    let edit = s.edit;
    ui::send_msg(edit, EM_SETSEL, 0, -1);
}

fn on_command(hwnd: HWND, id: usize, code: u16) {
    if code != BN_CLICKED {
        return;
    }

    match id {
        ID_OK => {
            let value = ui::get_text(st(hwnd).edit);
            let value = value.trim().to_string();
            let s = st(hwnd);
            s.value = value;
            s.accepted = true;
            ui::destroy_window(hwnd);
        }
        ID_CANCEL => ui::destroy_window(hwnd),
        _ => {}
    }
}
