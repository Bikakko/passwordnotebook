//! 设置对话框(含修改登录密码与重新生成恢复码)。

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use super::app;
use super::{dialog, dlg_recovery, dpapi, hello, sys::*, ui};

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
        760,
    );
}

fn st(hwnd: HWND) -> &'static mut SettingsState {
    unsafe { ui::state_ref::<SettingsState>(hwnd) }
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
    let ptr = unsafe { ui::create_param(lparam) } as *mut SettingsState;
    ui::set_user_data(hwnd, ptr as *mut std::ffi::c_void);
    let s = st(hwnd);
    let settings = app::state().settings.clone();

    label(hwnd, "解锁与安全", ID_SECTION_1, (20, 10, 200, 24));
    s.quick = ctl("BUTTON", "允许本机免密解锁", WS_TABSTOP | BS_AUTOCHECKBOX, 0, hwnd, ID_QUICK, (20, 36, 300, 24));
    label(
        hwnd,
        "用 Windows 凭据加密密钥缓存:系统未锁屏时打开程序无需输入登录密码;锁屏或屏保激活后缓存立即失效。",
        ID_QUICK_HINT,
        (40, 62, 500, 44),
    );
    s.hello = ctl(
        "BUTTON",
        "免密解锁时要求 Windows Hello 验证(指纹 / 人脸 / PIN)",
        WS_TABSTOP | BS_AUTOCHECKBOX,
        0,
        hwnd,
        ID_HELLO,
        (40, 108, 500, 24),
    );
    let available = hello::is_available();
    label(
        hwnd,
        if available {
            "此设备支持 Windows Hello。"
        } else {
            "此设备当前不可用 Windows Hello(未设置指纹/PIN 或硬件不支持),将回退为输入登录密码。"
        },
        ID_HELLO_HINT,
        (40, 132, 500, 24),
    );

    label(hwnd, "自动锁定", ID_SECTION_2, (20, 166, 200, 24));
    label(hwnd, "空闲多久后自动锁定密码本", ID_IDLE_LABEL, (20, 192, 300, 22));
    s.idle = ctl("COMBOBOX", "", WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST, 0, hwnd, ID_IDLE, (20, 214, 500, 200));
    for text in IDLE_LABELS {
        ui::combo_add(s.idle, text);
    }
    ui::combo_set_index(
        s.idle,
        IDLE_VALUES.iter().position(|v| *v == settings.idle_lock_minutes).unwrap_or(0) as i32,
    );

    label(hwnd, "剪贴板", ID_SECTION_3, (20, 248, 200, 24));
    label(hwnd, "复制密码后自动清空剪贴板", ID_CLIP_LABEL, (20, 274, 300, 22));
    s.clipboard = ctl("COMBOBOX", "", WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST, 0, hwnd, ID_CLIP, (20, 296, 500, 200));
    for text in CLIP_LABELS {
        ui::combo_add(s.clipboard, text);
    }
    ui::combo_set_index(
        s.clipboard,
        CLIP_VALUES
            .iter()
            .position(|v| *v == settings.clipboard_clear_seconds)
            .unwrap_or(2) as i32,
    );

    label(hwnd, "回收站", ID_SECTION_4, (20, 330, 200, 24));
    label(hwnd, "删除的记录保留时长(超期后下次解锁时自动彻底删除)", ID_BIN_LABEL, (20, 356, 500, 22));
    s.bin = ctl("COMBOBOX", "", WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST, 0, hwnd, ID_BIN, (20, 378, 500, 200));
    for text in BIN_LABELS {
        ui::combo_add(s.bin, text);
    }
    ui::combo_set_index(
        s.bin,
        BIN_VALUES
            .iter()
            .position(|v| *v == settings.bin_retention_days)
            .unwrap_or(2) as i32,
    );

    label(hwnd, "登录密码与恢复码", ID_SECTION_5, (20, 412, 200, 24));
    label(hwnd, "当前登录密码", ID_CUR_PW_LABEL, (20, 438, 200, 22));
    s.current_pw = ctl("EDIT", "", WS_BORDER | WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, hwnd, ID_CUR_PW, (20, 460, 500, 28));
    label(hwnd, "新的登录密码", ID_NEW_PW_LABEL, (20, 494, 200, 22));
    s.new_pw = ctl("EDIT", "", WS_BORDER | WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, hwnd, ID_NEW_PW, (20, 516, 500, 28));
    label(hwnd, "确认新的登录密码", ID_CONFIRM_PW_LABEL, (20, 550, 200, 22));
    s.confirm_pw = ctl("EDIT", "", WS_BORDER | WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, hwnd, ID_CONFIRM_PW, (20, 572, 500, 28));

    ctl("BUTTON", "修改登录密码", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_CHANGE_PW, (20, 608, 140, 34));
    ctl("BUTTON", "重新生成恢复码", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_REGEN, (172, 608, 160, 34));

    label(
        hwnd,
        &format!("数据库文件:{}", crate::paths::vault_display_path()),
        ID_PATH,
        (20, 648, 500, 22),
    );

    s.error = label(hwnd, "", ID_ERROR, (20, 672, 500, 22));
    ctl("BUTTON", "保存", WS_TABSTOP | BS_DEFPUSHBUTTON, 0, hwnd, ID_SAVE, (300, 700, 110, 36));
    ctl("BUTTON", "取消", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_CANCEL, (420, 700, 100, 36));

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

    on_quick_changed(hwnd);
    ui::set_focus(s.quick);
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
        always_on_top: app::state().settings.always_on_top,
    };

    // 设置加密写回库里(磁盘上不会出现额外的配置文件)。
    if let Err(e) = app::state().vault.update_settings(updated.clone()) {
        ui::set_text(st(hwnd).error, &e.to_string());
        return;
    }
    app::state().settings = updated.clone();

    // 让免密缓存的「是否允许 / 是否要求 Hello」立即生效。
    if updated.quick_unlock_enabled {
        if let Some((id, generation, dek)) = app::state().vault.quick_unlock_material() {
            dpapi::store(id, generation, &dek, updated.require_windows_hello);
        }
    } else {
        dpapi::clear();
    }

    ui::destroy_window(hwnd);
}

fn change_password(hwnd: HWND) {
    let s = st(hwnd);
    let current = ui::get_text(s.current_pw);
    let new = ui::get_text(s.new_pw);
    let confirm = ui::get_text(s.confirm_pw);

    if current.is_empty() {
        ui::set_text(s.error, "请输入当前登录密码。");
        return;
    }
    if new.chars().count() < 6 {
        ui::set_text(s.error, "新的登录密码太短,请至少使用 6 位字符。");
        return;
    }
    if new != confirm {
        ui::set_text(s.error, "两次输入的新登录密码不一致。");
        return;
    }

    ui::set_text(s.error, "正在修改…");
    ui::update_window(hwnd);

    match app::state().vault.change_master_password(&current, &new) {
        Ok(()) => {
            let settings = app::state().settings.clone();
            if let Some((id, generation, dek)) = app::state().vault.quick_unlock_material() {
                if settings.quick_unlock_enabled {
                    dpapi::store(id, generation, &dek, settings.require_windows_hello);
                }
            }
            let s = st(hwnd);
            ui::set_text(s.current_pw, "");
            ui::set_text(s.new_pw, "");
            ui::set_text(s.confirm_pw, "");
            ui::set_text(s.error, "登录密码已修改。");
            ui::info(hwnd, "登录密码已修改。", "完成");
        }
        Err(e) => ui::set_text(st(hwnd).error, &e.to_string()),
    }
}

fn regenerate_recovery(hwnd: HWND) {
    if !ui::confirm(
        hwnd,
        "重新生成后,旧的恢复码将立即失效。确定继续吗?",
        "重新生成恢复码",
    ) {
        return;
    }

    match app::state().vault.regenerate_recovery_code() {
        Ok(code) => {
            let settings = app::state().settings.clone();
            if let Some((id, generation, dek)) = app::state().vault.quick_unlock_material() {
                if settings.quick_unlock_enabled {
                    dpapi::store(id, generation, &dek, settings.require_windows_hello);
                }
            }
            ui::set_text(st(hwnd).error, "");
            dlg_recovery::show_code(hwnd, &code, false);
        }
        Err(e) => ui::set_text(st(hwnd).error, &format!("生成失败:{e}")),
    }
}
