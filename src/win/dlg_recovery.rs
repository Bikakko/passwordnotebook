//! 恢复码相关对话框:展示恢复码,以及用恢复码重设登录密码。

use std::path::Path;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::recovery;
use crate::vault::VaultService;

use super::app;
use super::{clipboard, dialog, sys::*, ui};

// ============ 展示恢复码 ============

const CLASS_CODE: &str = "PnbDlgRecoveryCode";

const C_HINT: usize = 1;
const C_CODE: usize = 2;
const C_COPY: usize = 3;
const C_SAVE: usize = 4;
const C_OK: usize = 5;

struct CodeState {
    code: String,
}

pub fn show_code(owner: HWND, code: &str, initial: bool) {
    let state = Box::new(CodeState {
        code: code.to_string(),
    });
    dialog::open(CLASS_CODE, if initial { "请保存恢复码" } else { "新的恢复码" }, owner, code_proc, state, 560, 400);
}

unsafe extern "system" fn code_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            let ptr = unsafe { ui::create_param(lparam) } as *mut CodeState;
            ui::set_user_data(hwnd, ptr as *mut std::ffi::c_void);
            let s = unsafe { &*ptr };

            ctl(
                "STATIC",
                "这是找回登录密码的唯一凭证。请抄写或保存到其他安全的地方;忘记登录密码且丢失恢复码后,数据将无法恢复。",
                SS_LEFT,
                0,
                hwnd,
                C_HINT,
                (20, 16, 504, 96),
            );
            let code_edit = ctl(
                "EDIT",
                &s.code,
                WS_BORDER | WS_TABSTOP | ES_READONLY | ES_AUTOHSCROLL,
                WS_EX_CLIENTEDGE,
                hwnd,
                C_CODE,
                (20, 122, 504, 42),
            );
            ctl("BUTTON", "复制恢复码", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, C_COPY, (20, 180, 150, 36));
            ctl("BUTTON", "另存为文本…", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, C_SAVE, (182, 180, 160, 36));
            let ok = ctl("BUTTON", "我已妥善保存", WS_TABSTOP | BS_DEFPUSHBUTTON, 0, hwnd, C_OK, (20, 288, 190, 38));
            ctl(
                "STATIC",
                "提示:恢复码不区分大小写,可省略连字符。",
                SS_LEFT,
                0,
                hwnd,
                C_HINT + 100,
                (20, 238, 504, 26),
            );

            let font = app::state().font;
            ui::apply_font_to(
                hwnd,
                &[C_HINT, C_CODE, C_COPY, C_SAVE, C_OK, C_HINT + 100],
                font,
            );
            ui::set_focus(ok);
            let _ = code_edit;
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as usize;
            let s = unsafe { &*ui::user_data::<CodeState>(hwnd) };
            match id {
                C_COPY => {
                    clipboard::set_text(&s.code);
                }
                C_SAVE => save_code(hwnd, &s.code),
                C_OK => ui::destroy_window(hwnd),
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
        "Password Notebook 恢复码\r\n生成时间:{}\r\n\r\n{}\r\n\r\n提示:忘记登录密码时,用此恢复码即可重设登录密码。请妥善保管。\r\n",
        super::timefmt::local_string(crate::model::now_secs()),
        code
    );

    match std::fs::write(&path, content) {
        Ok(()) => ui::info(hwnd, "已保存。", "完成"),
        Err(e) => ui::error(hwnd, &format!("保存失败:{e}"), "错误"),
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
            let ptr = unsafe { ui::create_param(lparam) } as *mut RecoverState;
            ui::set_user_data(hwnd, ptr as *mut std::ffi::c_void);
            let s = unsafe { &mut *ptr };

            ctl(
                "STATIC",
                "输入创建密码本时保存的恢复码,并设置新的登录密码。恢复码不区分大小写,可省略连字符。",
                SS_LEFT,
                0,
                hwnd,
                R_HINT,
                (20, 14, 484, 48),
            );
            ctl("STATIC", "恢复码", SS_LEFT, 0, hwnd, R_CODE_LABEL, (20, 68, 100, 22));
            s.code = ctl(
                "EDIT",
                "",
                WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL,
                WS_EX_CLIENTEDGE,
                hwnd,
                R_CODE,
                (20, 92, 484, 30),
            );
            ctl("STATIC", "新的登录密码", SS_LEFT, 0, hwnd, R_PW1_LABEL, (20, 134, 200, 22));
            s.pw1 = ctl(
                "EDIT",
                "",
                WS_BORDER | WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL,
                WS_EX_CLIENTEDGE,
                hwnd,
                R_PW1,
                (20, 158, 484, 30),
            );
            ctl("STATIC", "确认新的登录密码", SS_LEFT, 0, hwnd, R_PW2_LABEL, (20, 200, 200, 22));
            s.pw2 = ctl(
                "EDIT",
                "",
                WS_BORDER | WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL,
                WS_EX_CLIENTEDGE,
                hwnd,
                R_PW2,
                (20, 224, 484, 30),
            );
            s.error = ctl("STATIC", "", SS_LEFT, 0, hwnd, R_ERROR, (20, 262, 484, 40));
            s.submit = ctl("BUTTON", "重设登录密码并解锁", WS_TABSTOP | BS_DEFPUSHBUTTON, 0, hwnd, R_SUBMIT, (20, 312, 200, 38));
            ctl("BUTTON", "取消", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, R_CANCEL, (232, 312, 100, 38));

            let font = app::state().font;
            ui::apply_font_to(
                hwnd,
                &[R_HINT, R_CODE_LABEL, R_CODE, R_PW1_LABEL, R_PW1, R_PW2_LABEL, R_PW2, R_ERROR, R_SUBMIT, R_CANCEL],
                font,
            );
            ui::set_focus(s.code);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as usize;
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
    let code = ui::get_text(s.code);
    let pw1 = ui::get_text(s.pw1);
    let pw2 = ui::get_text(s.pw2);

    if !recovery::is_valid(&code) {
        ui::set_text(s.error, "恢复码格式不正确,应为 32 位字符(可含连字符)。");
        return;
    }
    if pw1.chars().count() < 6 {
        ui::set_text(s.error, "新的登录密码太短,请至少使用 6 位字符。");
        return;
    }
    if pw1 != pw2 {
        ui::set_text(s.error, "两次输入的新登录密码不一致。");
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
        ui::set_text(st_recover(hwnd).error, &format!("重设失败:{e}"));
        return;
    }

    st_recover(hwnd).accepted = true;
    ui::destroy_window(hwnd);
}

fn st_recover(hwnd: HWND) -> &'static mut RecoverState {
    unsafe { ui::state_ref::<RecoverState>(hwnd) }
}

/// 对话框内创建控件的通用辅助。
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

#[allow(dead_code)]
fn unused_vault(_: &VaultService) {}
