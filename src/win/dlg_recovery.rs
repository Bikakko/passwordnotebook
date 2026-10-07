//! 恢复码相关对话框:展示恢复码,以及用恢复码重设登录密码。

use std::path::Path;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::recovery;

use super::app;
use super::{clipboard, dialog, sys::*, ui::{self, ctl}};

// ============ 展示恢复码 ============

const CLASS_CODE: &str = "PnbDlgRecoveryCode";

const C_HINT: usize = 1;
const C_CODE: usize = 2;
const C_COPY: usize = 3;
const C_SAVE: usize = 4;
const C_OK: usize = 5;
const C_CONFIRM: usize = 6;

struct CodeState {
    code: String,
    confirm: HWND,
    ok: HWND,
}

pub fn show_code(owner: HWND, code: &str, initial: bool) {
    let state = Box::new(CodeState {
        code: code.to_string(),
        confirm: HWND::default(),
        ok: HWND::default(),
    });
    dialog::open(
        CLASS_CODE,
        if initial { "请保存恢复码" } else { "新的恢复码" },
        owner,
        code_proc,
        state,
        560,
        312,
    );
}

unsafe extern "system" fn code_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            unsafe { ui::attach_state::<CodeState>(hwnd, lparam) };
            let s = st_code(hwnd);

            ctl(
                "STATIC",
                "请保存恢复码，这是登录密码的唯一找回方式。",
                SS_LEFT,
                0,
                hwnd,
                C_HINT,
                (20, 16, 504, 44),
            );
            // 用多行显示:恢复码有 39 个字符,单行会被截断,用户照着抄就抄不全。
            let code_edit = ctl(
                "EDIT",
                &s.code,
                WS_BORDER | WS_TABSTOP | ES_MULTILINE | ES_READONLY,
                WS_EX_CLIENTEDGE,
                hwnd,
                C_CODE,
                (20, 70, 504, 60),
            );
            ctl("BUTTON", "复制恢复码", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, C_COPY, (20, 142, 150, 36));
            ctl("BUTTON", "另存为文本…", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, C_SAVE, (182, 142, 160, 36));
            // 必须显式确认已保存,否则「完成」按钮不可用 ——
            // 避免用户在没抄下来的情况下一路点完,之后忘记登录密码就真进不去了。
            s.confirm = ctl(
                "BUTTON",
                "我已妥善保存恢复码",
                WS_TABSTOP | BS_AUTOCHECKBOX,
                0,
                hwnd,
                C_CONFIRM,
                (20, 192, 460, 30),
            );
            s.ok = ctl(
                "BUTTON",
                "完成",
                WS_TABSTOP | BS_DEFPUSHBUTTON,
                0,
                hwnd,
                C_OK,
                (330, 234, 210, 36),
            );
            ui::enable(s.ok, false);

            let font = app::state().font;
            ui::apply_font_to(hwnd, &[C_HINT, C_CODE, C_COPY, C_SAVE, C_OK, C_CONFIRM], font);
            ui::set_focus(s.confirm);
            let _ = code_edit;
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => LRESULT(ui::static_label_reply(wparam.0)),
        WM_COMMAND => {
            // 注意:id 相同的不同控件会发不同通知码,必须一起判断。
            // (只读输入框的 id 恰好也是 2,它发的 EN_CHANGE 曾被误当成 IDCANCEL。)
            let (id, code) = dialog::command_params(wparam);
            let s = st_code(hwnd);
            match (id, code) {
                (C_COPY, BN_CLICKED) => {
                    clipboard::set_text(&s.code);
                }
                (C_SAVE, BN_CLICKED) => save_code(hwnd, &s.code),
                (C_CONFIRM, BN_CLICKED) => ui::enable(s.ok, ui::is_checked(s.confirm)),
                (C_OK, BN_CLICKED) => ui::destroy_window(hwnd),
                (IDCANCEL, BN_CLICKED) => close_with_warning(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            close_with_warning(hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn st_code(hwnd: HWND) -> &'static mut CodeState {
    unsafe { ui::state_ref::<CodeState>(hwnd) }
}

/// 直接关闭(点 X 或按 ESC)时的兜底提醒:恢复码没抄下来就关,风险很大。
fn close_with_warning(hwnd: HWND) {
    let s = st_code(hwnd);
    if !ui::is_checked(s.confirm) && !ui::confirm(hwnd, RECOVERY_UNSAVED_MESSAGE, "注意") {
        return;
    }
    ui::destroy_window(hwnd);
}

/// 恢复码还没保存就关闭时的确认文案。
const RECOVERY_UNSAVED_MESSAGE: &str = "恢复码还没保存，关闭后无法再查看。确定关闭吗？";

fn save_code(hwnd: HWND, code: &str) {
    let Some(path) = ui::pick_file(
        hwnd,
        true,
        &[("文本文件", "*.txt"), ("所有文件", "*.*")],
        "密码本恢复码.txt",
    ) else {
        return;
    };

    let content = format!(
        "PasswordNotebook 恢复码\r\n生成时间：{}\r\n\r\n{}\r\n\r\n忘记登录密码时，用此恢复码重设登录密码。\r\n",
        super::timefmt::local_string(crate::model::now_secs()),
        code
    );

    match std::fs::write(&path, content) {
        Ok(()) => ui::info(hwnd, "恢复码已保存。", "保存完成"),
        Err(e) => ui::error(hwnd, &format!("保存失败（{e}）"), "保存失败"),
    }
}

// ============ 用恢复码找回 ============

const CLASS_RECOVER: &str = "PnbDlgRecover";

const R_HINT: usize = 1;
const R_CODE_LABEL: usize = 2;
const R_CODE: usize = 3;
const R_PW1_LABEL: usize = 4;
const R_PW1: usize = 5;
const R_PW2_LABEL: usize = 6;
const R_PW2: usize = 7;
const R_ERROR: usize = 8;
const R_SUBMIT: usize = 9;
const R_CANCEL: usize = 10;

struct RecoverState {
    vault_path: String,
    accepted: bool,
    code: HWND,
    pw1: HWND,
    pw2: HWND,
    error: HWND,
    submit: HWND,
}

pub fn show_recover(owner: HWND, vault_path: &str) -> bool {
    let state = Box::new(RecoverState {
        vault_path: vault_path.to_string(),
        accepted: false,
        code: HWND::default(),
        pw1: HWND::default(),
        pw2: HWND::default(),
        error: HWND::default(),
        submit: HWND::default(),
    });

    let state = dialog::open(CLASS_RECOVER, "找回登录密码", owner, recover_proc, state, 540, 420);
    state.accepted
}

unsafe extern "system" fn recover_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            unsafe { ui::attach_state::<RecoverState>(hwnd, lparam) };
            let s = st_recover(hwnd);

            ctl(
                "STATIC",
                "输入恢复码和新登录密码。恢复码不区分大小写，可省略连字符。",
                SS_LEFT,
                0,
                hwnd,
                R_HINT,
                (20, 14, 500, 48),
            );
            ctl("STATIC", "恢复码", SS_LEFT, 0, hwnd, R_CODE_LABEL, (20, 68, 100, 22));
            s.code = ctl(
                "EDIT",
                "",
                WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL,
                WS_EX_CLIENTEDGE,
                hwnd,
                R_CODE,
                (20, 92, 500, 30),
            );
            ctl("STATIC", "新的登录密码", SS_LEFT, 0, hwnd, R_PW1_LABEL, (20, 134, 200, 22));
            s.pw1 = ctl(
                "EDIT",
                "",
                WS_BORDER | WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL,
                WS_EX_CLIENTEDGE,
                hwnd,
                R_PW1,
                (20, 158, 500, 30),
            );
            ctl("STATIC", "确认新的登录密码", SS_LEFT, 0, hwnd, R_PW2_LABEL, (20, 200, 200, 22));
            s.pw2 = ctl(
                "EDIT",
                "",
                WS_BORDER | WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL,
                WS_EX_CLIENTEDGE,
                hwnd,
                R_PW2,
                (20, 224, 500, 30),
            );
            s.error = ctl("STATIC", "", SS_LEFT, 0, hwnd, R_ERROR, (20, 262, 500, 40));
            s.submit = ctl("BUTTON", "重设登录密码并解锁", WS_TABSTOP | BS_DEFPUSHBUTTON, 0, hwnd, R_SUBMIT, (212, 312, 200, 36));
            ctl("BUTTON", "取消", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, R_CANCEL, (420, 312, 100, 36));

            let font = app::state().font;
            ui::apply_font_to(
                hwnd,
                &[R_HINT, R_CODE_LABEL, R_CODE, R_PW1_LABEL, R_PW1, R_PW2_LABEL, R_PW2, R_ERROR, R_SUBMIT, R_CANCEL],
                font,
            );
            ui::set_focus(s.code);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => LRESULT(ui::static_label_reply(wparam.0)),
        WM_COMMAND => {
            let (id, _) = dialog::command_params(wparam);
            match id {
                R_SUBMIT => do_reset(hwnd),
                R_CANCEL => ui::destroy_window(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            ui::destroy_window(hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn do_reset(hwnd: HWND) {
    let s = st_recover(hwnd);
    let code = ui::get_secret(s.code);
    let pw1 = ui::get_secret(s.pw1);
    let pw2 = ui::get_secret(s.pw2);

    if !recovery::is_valid(&code) {
        ui::set_text(s.error, "恢复码格式不正确，应为 32 位字符（可含连字符）。");
        return;
    }
    if let Err(message) = super::main_ui::validate_new_password("新的登录密码", &pw1, &pw2) {
        ui::set_text(s.error, &message);
        return;
    }

    ui::set_text(s.error, "正在重设…");
    ui::set_text(s.submit, "正在重设…");
    ui::update_window(hwnd);

    let path = s.vault_path.clone();
    let vault = &mut app::state().vault;

    if let Err(e) = vault.open_with_recovery_code(Path::new(&path), &code) {
        ui::set_text(st_recover(hwnd).submit, "重设登录密码并解锁");
        ui::set_text(st_recover(hwnd).error, &e.to_string());
        return;
    }
    if let Err(e) = vault.reset_master_password(&pw1) {
        ui::set_text(st_recover(hwnd).submit, "重设登录密码并解锁");
        ui::set_text(st_recover(hwnd).error, &format!("重设登录密码失败（{e}）"));
        return;
    }

    st_recover(hwnd).accepted = true;
    ui::destroy_window(hwnd);
}

fn st_recover(hwnd: HWND) -> &'static mut RecoverState {
    unsafe { ui::state_ref::<RecoverState>(hwnd) }
}
