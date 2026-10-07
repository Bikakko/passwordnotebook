//! 设置对话框(含修改登录密码与重新生成恢复码)。

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use super::app;
use super::tokens::*;
use super::{dialog, dlg_recovery, dpapi, hello, sys::*, ui::{self, ctl, label}};

const CLASS: &str = "PnbDlgSettings";

const ID_QUICK: usize = 1;
const ID_QUICK_HINT: usize = 2;
const ID_HELLO: usize = 3;
const ID_HELLO_HINT: usize = 4;
const ID_IDLE_LABEL: usize = 5;
const ID_IDLE: usize = 6;
const ID_CLIP_LABEL: usize = 7;
const ID_CLIP: usize = 8;
const ID_BIN_LABEL: usize = 9;
const ID_BIN: usize = 10;
const ID_CUR_PW_LABEL: usize = 11;
const ID_CUR_PW: usize = 12;
const ID_NEW_PW_LABEL: usize = 13;
const ID_NEW_PW: usize = 14;
const ID_CONFIRM_PW_LABEL: usize = 15;
const ID_CONFIRM_PW: usize = 16;
const ID_CHANGE_PW: usize = 17;
const ID_REGEN: usize = 18;
const ID_PATH: usize = 19;
const ID_ERROR: usize = 20;
const ID_SAVE: usize = 21;
const ID_CANCEL: usize = 22;
const ID_SECTION_1: usize = 100;
const ID_SECTION_2: usize = 101;
const ID_SECTION_3: usize = 102;
const ID_SECTION_4: usize = 103;
const ID_SECTION_5: usize = 104;

const IDLE_LABELS: [&str; 6] = [
    "跟随系统屏保",
    "不自动锁定",
    "空闲 5 分钟",
    "空闲 15 分钟",
    "空闲 30 分钟",
    "空闲 60 分钟",
];
const IDLE_VALUES: [i32; 6] = [0, -1, 5, 15, 30, 60];

const CLIP_LABELS: [&str; 5] = ["不自动清空", "10 秒", "20 秒", "30 秒", "60 秒"];
const CLIP_VALUES: [u32; 5] = [0, 10, 20, 30, 60];

const BIN_LABELS: [&str; 4] = ["永久保留", "7 天", "30 天", "90 天"];
const BIN_VALUES: [i64; 4] = [0, 7, 30, 90];

struct SettingsState {
    quick: HWND,
    hello: HWND,
    idle: HWND,
    clipboard: HWND,
    bin: HWND,
    current_pw: HWND,
    new_pw: HWND,
    confirm_pw: HWND,
    error: HWND,
}

pub fn show(owner: HWND) {
    let state = Box::new(SettingsState {
        quick: HWND::default(),
        hello: HWND::default(),
        idle: HWND::default(),
        clipboard: HWND::default(),
        bin: HWND::default(),
        current_pw: HWND::default(),
        new_pw: HWND::default(),
        confirm_pw: HWND::default(),
        error: HWND::default(),
    });
    let _ = dialog::open(
        CLASS,
        &format!("设置 — {}", super::app_title()),
        owner,
        wnd_proc,
        state,
        560,
        776,
    );
}

fn st(hwnd: HWND) -> &'static mut SettingsState {
    unsafe { ui::state_ref::<SettingsState>(hwnd) }
}

/// 路径标签放不下时的缩略:只保留末尾,前面用省略号。
fn short_path(path: &str) -> String {
    const MAX_CHARS: usize = 46;
    let count = path.chars().count();
    if count <= MAX_CHARS {
        return path.to_string();
    }
    let tail: String = path.chars().skip(count - (MAX_CHARS - 1)).collect();
    format!("…{tail}")
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
    unsafe { ui::attach_state::<SettingsState>(hwnd, lparam) };
    let s = st(hwnd);
    let settings = app::state().settings.clone();

    label(hwnd, "解锁与安全", ID_SECTION_1, (MARGIN, 20, 200, SECTION_TITLE_H));
    s.quick = ctl("BUTTON", "允许本机免密解锁", WS_TABSTOP | BS_AUTOCHECKBOX, 0, hwnd, ID_QUICK, (MARGIN, 36, 300, CHECK_H));
    label(
        hwnd,
        "用系统凭据加密密钥并缓存：未锁屏时打开程序无需输入登录密码，锁屏或屏保后失效。",
        ID_QUICK_HINT,
        (40, 62, 480, 44),
    );
    s.hello = ctl(
        "BUTTON",
        "免密解锁时要求 Windows Hello 验证（指纹/人脸/PIN）",
        WS_TABSTOP | BS_AUTOCHECKBOX,
        0,
        hwnd,
        ID_HELLO,
        (40, 108, 480, CHECK_H),
    );
    // 启动时已在后台测过;到这儿一般已有结果,没有就暂按「不可用」显示。
    let available = hello::availability().unwrap_or(false);
    label(
        hwnd,
        if available {
            "此设备支持 Windows Hello。"
        } else {
            "此设备不支持 Windows Hello，改用登录密码。"
        },
        ID_HELLO_HINT,
        (40, 132, 480, LABEL_H),
    );

    label(hwnd, "自动锁定", ID_SECTION_2, (MARGIN, 162, 200, SECTION_TITLE_H));
    label(hwnd, "空闲多久后自动锁定密码本", ID_IDLE_LABEL, (MARGIN, 188, 300, LABEL_H));
    s.idle = ctl("COMBOBOX", "", WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST, 0, hwnd, ID_IDLE, (MARGIN, 210, 520, COMBO_DROP_H));
    fill_combo(s.idle, &IDLE_LABELS, &IDLE_VALUES, settings.idle_lock_minutes, 0);

    label(hwnd, "剪贴板", ID_SECTION_3, (MARGIN, 240, 200, SECTION_TITLE_H));
    label(hwnd, "复制密码后自动清空剪贴板", ID_CLIP_LABEL, (MARGIN, 266, 300, LABEL_H));
    s.clipboard = ctl("COMBOBOX", "", WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST, 0, hwnd, ID_CLIP, (MARGIN, 288, 520, COMBO_DROP_H));
    fill_combo(s.clipboard, &CLIP_LABELS, &CLIP_VALUES, settings.clipboard_clear_seconds, 2);

    label(hwnd, "回收站", ID_SECTION_4, (MARGIN, 318, 200, SECTION_TITLE_H));
    label(hwnd, "回收站条目保留时长（超期后下次解锁时彻底删除）", ID_BIN_LABEL, (MARGIN, 344, 520, LABEL_H));
    s.bin = ctl("COMBOBOX", "", WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST, 0, hwnd, ID_BIN, (MARGIN, 366, 520, COMBO_DROP_H));
    fill_combo(s.bin, &BIN_LABELS, &BIN_VALUES, settings.bin_retention_days, 2);

    label(hwnd, "登录密码与恢复码", ID_SECTION_5, (MARGIN, 396, 200, SECTION_TITLE_H));
    label(hwnd, "当前登录密码", ID_CUR_PW_LABEL, (MARGIN, 422, 200, LABEL_H));
    s.current_pw = ctl("EDIT", "", WS_BORDER | WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, hwnd, ID_CUR_PW, (MARGIN, 444, 520, FIELD_H));
    label(hwnd, "新的登录密码", ID_NEW_PW_LABEL, (MARGIN, 478, 200, LABEL_H));
    s.new_pw = ctl("EDIT", "", WS_BORDER | WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, hwnd, ID_NEW_PW, (MARGIN, 500, 520, FIELD_H));
    label(hwnd, "确认新的登录密码", ID_CONFIRM_PW_LABEL, (MARGIN, 534, 200, LABEL_H));
    s.confirm_pw = ctl("EDIT", "", WS_BORDER | WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, hwnd, ID_CONFIRM_PW, (MARGIN, 556, 520, FIELD_H));

    ctl("BUTTON", "修改登录密码", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_CHANGE_PW, (MARGIN, 592, 140, BUTTON_H));
    ctl("BUTTON", "重新生成恢复码", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_REGEN, (168, 592, 160, BUTTON_H));

    label(
        hwnd,
        &format!("密码本文件：{}", short_path(&crate::paths::vault_display_path())),
        ID_PATH,
        (MARGIN, 632, 520, LABEL_H),
    );

    s.error = label(hwnd, "", ID_ERROR, (MARGIN, 658, 520, 36));
    ctl("BUTTON", "保存", WS_TABSTOP | BS_DEFPUSHBUTTON, 0, hwnd, ID_SAVE, (322, 700, 110, BUTTON_H));
    ctl("BUTTON", "取消", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_CANCEL, (440, 700, 100, BUTTON_H));

    ui::set_checked(s.quick, settings.quick_unlock_enabled);
    ui::set_checked(s.hello, settings.require_windows_hello);

    let font = app::state().font;
    ui::apply_font_to(
        hwnd,
        &[
            ID_QUICK, ID_QUICK_HINT, ID_HELLO, ID_HELLO_HINT, ID_IDLE_LABEL, ID_IDLE, ID_CLIP_LABEL,
            ID_CLIP, ID_BIN_LABEL, ID_BIN, ID_CUR_PW_LABEL, ID_CUR_PW, ID_NEW_PW_LABEL, ID_NEW_PW,
            ID_CONFIRM_PW_LABEL, ID_CONFIRM_PW, ID_CHANGE_PW, ID_REGEN, ID_PATH, ID_ERROR, ID_SAVE,
            ID_CANCEL,
            // 分节标题(必须一起套用字体,否则会比正文小一号)
            ID_SECTION_1, ID_SECTION_2, ID_SECTION_3, ID_SECTION_4, ID_SECTION_5,
        ],
        font,
    );

    // 区块标题改用粗体,与正文拉开层级。
    ui::apply_font_to(
        hwnd,
        &[ID_SECTION_1, ID_SECTION_2, ID_SECTION_3, ID_SECTION_4, ID_SECTION_5],
        app::state().font_bold,
    );

    on_quick_changed(hwnd);
    show_cache_state(hwnd);
    // on_quick_changed 会重新 st(hwnd),不能复用上面那把引用。
    ui::set_focus(st(hwnd).quick);
}

/// 免密缓存当前是否真的可用。
///
/// 「设置里勾着」不等于「缓存写得进去」:目录被安全策略拒绝、磁盘满、
/// 账户配置异常都会让写入失败。用户勾了却永远不生效、界面还一声不吭,
/// 是在骗人 —— 打开设置时就把实情摆出来。
fn show_cache_state(hwnd: HWND) {
    if !app::state().settings.quick_unlock_enabled {
        return;
    }
    let Some((id, generation, _)) = app::state().vault.quick_unlock_material() else {
        return;
    };
    if dpapi::load(id, generation).is_none() {
        ui::set_text(
            st(hwnd).error,
            "免密缓存不可用，下次启动仍需输入登录密码（点「保存」重试）。",
        );
    }
}

fn on_command(hwnd: HWND, id: usize, code: u16) {
    match id {
        ID_QUICK if code == BN_CLICKED => on_quick_changed(hwnd),
        ID_SAVE if code == BN_CLICKED => save(hwnd),
        ID_CANCEL if code == BN_CLICKED => ui::destroy_window(hwnd),
        ID_CHANGE_PW if code == BN_CLICKED => change_password(hwnd),
        ID_REGEN if code == BN_CLICKED => regenerate_recovery(hwnd),
        _ => {}
    }
}

fn on_quick_changed(hwnd: HWND) {
    let s = st(hwnd);
    let enabled = ui::is_checked(s.quick);
    ui::enable(s.hello, enabled);
    if !enabled {
        ui::set_checked(s.hello, false);
    }
}

/// 填充下拉框并按值选中(表里找不到当前值时退回 `fallback` 下标)。
fn fill_combo<T: PartialEq + Copy>(
    combo: HWND,
    labels: &[&str],
    values: &[T],
    current: T,
    fallback: usize,
) {
    for text in labels {
        ui::combo_add(combo, text);
    }
    let index = values.iter().position(|v| *v == current).unwrap_or(fallback);
    ui::combo_set_index(combo, index as i32);
}

fn save(hwnd: HWND) {
    let s = st(hwnd);
    let idle_index = ui::combo_index(s.idle).max(0) as usize;
    let clip_index = ui::combo_index(s.clipboard).max(0) as usize;
    let bin_index = ui::combo_index(s.bin).max(0) as usize;

    let updated = crate::model::Settings {
        quick_unlock_enabled: ui::is_checked(s.quick),
        require_windows_hello: ui::is_checked(s.hello),
        idle_lock_minutes: IDLE_VALUES.get(idle_index).copied().unwrap_or(0),
        clipboard_clear_seconds: CLIP_VALUES.get(clip_index).copied().unwrap_or(20),
        bin_retention_days: BIN_VALUES.get(bin_index).copied().unwrap_or(30),
        favorites_only: app::state().settings.favorites_only,
        column_widths: app::state().settings.column_widths.clone(),
    };

    // 设置加密写回库里(磁盘上不会出现额外的配置文件)。
    if let Err(e) = app::state().vault.update_settings(updated.clone()) {
        // 写盘失败时设置已改在内存里,只是没进文件 —— 这里也要如实说明。
        ui::set_text(st(hwnd).error, &super::main_ui::save_failure_inline(&e));
        return;
    }
    app::state().settings = updated.clone();

    // 让免密缓存的「是否允许 / 是否要求 Hello」立即生效。
    //
    // 写入失败必须说出来:缓存没落地却显示「已开启」,下次启动用户会发现
    // 免密解锁根本没生效,而界面从未提过一个字。失败时**不关闭**对话框,
    // 让用户看到原因,自己决定是留着还是关掉这个开关。
    let mut cache_error = None;
    if updated.quick_unlock_enabled {
        if let Some((id, generation, dek)) = app::state().vault.quick_unlock_material()
            && let Err(e) = dpapi::store(id, generation, &dek, updated.require_windows_hello)
        {
            cache_error = Some(e);
        }
    } else {
        dpapi::clear();
    }

    if let Some(e) = cache_error {
        ui::set_text(
            st(hwnd).error,
            &format!("设置已保存，但免密缓存写入失败，下次仍需输入登录密码：{e}"),
        );
        return;
    }

    ui::destroy_window(hwnd);
}

/// 按当前设置重写一次免密缓存;失败返回原因,成功或未启用返回 `None`。
fn refresh_cache() -> Option<String> {
    if !app::state().settings.quick_unlock_enabled {
        return None;
    }
    let (id, generation, dek) = app::state().vault.quick_unlock_material()?;
    let require_hello = app::state().settings.require_windows_hello;
    dpapi::store(id, generation, &dek, require_hello).err()
}

fn change_password(hwnd: HWND) {
    let s = st(hwnd);
    let current = ui::get_secret(s.current_pw);
    let new = ui::get_secret(s.new_pw);
    let confirm = ui::get_secret(s.confirm_pw);

    if current.is_empty() {
        ui::set_text(s.error, "请输入当前登录密码。");
        return;
    }
    if let Err(message) = super::main_ui::validate_new_password("新的登录密码", &new, &confirm) {
        ui::set_text(s.error, &message);
        return;
    }

    ui::set_text(s.error, "正在修改…");
    ui::update_window(hwnd);

    match app::state().vault.change_master_password(&current, &new) {
        Ok(()) => {
            // 改密会让旧缓存作废,这里用新密钥重写一份;写失败也要说。
            let cache_warning = refresh_cache();
            let s = st(hwnd);
            ui::set_text(s.current_pw, "");
            ui::set_text(s.new_pw, "");
            ui::set_text(s.confirm_pw, "");
            ui::set_text(s.error, "登录密码已修改。");

            let message = match cache_warning {
                Some(e) => format!("登录密码已修改。\n\n但免密缓存写入失败，下次仍需输入登录密码：{e}"),
                None => "登录密码已修改。".to_string(),
            };
            ui::info(hwnd, &message, "修改登录密码完成");
        }
        Err(e) => ui::set_text(st(hwnd).error, &e.to_string()),
    }
}

fn regenerate_recovery(hwnd: HWND) {
    if !ui::confirm(
        hwnd,
        "重新生成后，旧的恢复码将立即失效。确定继续吗？",
        "重新生成恢复码",
    ) {
        return;
    }

    match app::state().vault.regenerate_recovery_code() {
        Ok(code) => {
            let cache_warning = refresh_cache();
            let status = match cache_warning {
                Some(e) => format!("恢复码已更新，但免密缓存写入失败：{e}"),
                None => String::new(),
            };
            ui::set_text(st(hwnd).error, &status);
            dlg_recovery::show_code(hwnd, &code, false);
        }
        Err(e) => ui::set_text(st(hwnd).error, &format!("生成失败：{e}")),
    }
}
