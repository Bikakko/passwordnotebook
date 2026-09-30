//! 导入 / 导出对话框。
//!
//! 两条通道的定位不同,界面上也刻意分开:
//!
//! - **导出**:默认包含明文密码,但必须由用户显式勾选确认;不勾选密码时导出的是一份
//!   「只有账号信息」的清单,可以放心贴在工单或共享文档里。
//! - **导入**:先解析、再让用户确认、然后自动备份库文件,最后才写库。

use std::path::Path;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::export_import::{self, DuplicateStrategy, ExportOptions, Format};

use super::app;
use super::{dialog, sys::*, timefmt, ui};

const CLASS: &str = "PnbDlgTransfer";

const ID_HINT: usize = 1;

const ID_EXPORT_GROUP: usize = 10;
const ID_EXPORT_FORMAT_LABEL: usize = 11;
const ID_EXPORT_FORMAT: usize = 12;
const ID_EXPORT_PASSWORDS: usize = 13;
const ID_EXPORT_ACK: usize = 14;
const ID_EXPORT_BTN: usize = 15;
const ID_EXPORT_NOTE: usize = 16;
const ID_EXPORT_FOOTNOTE: usize = 17;

const ID_IMPORT_GROUP: usize = 20;
const ID_IMPORT_STRATEGY_LABEL: usize = 21;
const ID_IMPORT_STRATEGY: usize = 22;
const ID_IMPORT_HINT: usize = 23;
const ID_IMPORT_BTN: usize = 24;
const ID_IMPORT_FOOTNOTE: usize = 25;

const ID_STATUS: usize = 30;
const ID_CLOSE: usize = 31;

/// 需要套用界面字体的控件(逐个列出,而不是猜 id 区间)。
const CONTROL_IDS: [usize; 17] = [
    ID_HINT,
    ID_EXPORT_GROUP,
    ID_EXPORT_FORMAT_LABEL,
    ID_EXPORT_FORMAT,
    ID_EXPORT_PASSWORDS,
    ID_EXPORT_ACK,
    ID_EXPORT_BTN,
    ID_EXPORT_NOTE,
    ID_EXPORT_FOOTNOTE,
    ID_IMPORT_GROUP,
    ID_IMPORT_STRATEGY_LABEL,
    ID_IMPORT_STRATEGY,
    ID_IMPORT_HINT,
    ID_IMPORT_BTN,
    ID_IMPORT_FOOTNOTE,
    ID_STATUS,
    ID_CLOSE,
];

struct TransferState {
    format: HWND,
    passwords: HWND,
    ack: HWND,
    strategy: HWND,
    status: HWND,
    /// 本次打开对话框期间是否真的改动过库(供调用方决定要不要刷新列表)。
    changed: bool,
}

/// 打开对话框,返回期间是否改动过库。
pub fn show(owner: HWND) -> bool {
    let state = Box::new(TransferState {
        format: HWND::default(),
        passwords: HWND::default(),
        ack: HWND::default(),
        strategy: HWND::default(),
        status: HWND::default(),
        changed: false,
    });

    let state = dialog::open(CLASS, "导入 / 导出", owner, wnd_proc, state, 680, 520);
    state.changed
}

fn st(hwnd: HWND) -> &'static mut TransferState {
    unsafe { ui::state_ref::<TransferState>(hwnd) }
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

fn combo(parent: HWND, id: usize, r: (i32, i32, i32, i32)) -> HWND {
    ctl(
        "COMBOBOX",
        "",
        WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST,
        0,
        parent,
        id,
        r,
    )
}

fn on_create(hwnd: HWND, lparam: LPARAM) {
    let ptr = unsafe { ui::create_param(lparam) } as *mut TransferState;
    ui::set_user_data(hwnd, ptr as *mut std::ffi::c_void);
    let s = st(hwnd);

    ctl(
        "STATIC",
        "导出可用于备份或迁移到别的密码管理器;导入支持本程序导出的文件,也能直接读入 Chrome、Edge、Bitwarden、1Password、KeePass、LastPass 导出的 CSV。",
        SS_LEFT,
        0,
        hwnd,
        ID_HINT,
        (20, 14, 620, 42),
    );

    // ---- 导出 ----
    ctl("BUTTON", "导出", BS_GROUPBOX, 0, hwnd, ID_EXPORT_GROUP, (20, 62, 300, 320));
    ctl("STATIC", "文件格式", SS_LEFT, 0, hwnd, ID_EXPORT_FORMAT_LABEL, (36, 88, 80, 22));
    s.format = combo(hwnd, ID_EXPORT_FORMAT, (120, 84, 184, 200));
    s.passwords = ctl(
        "BUTTON",
        "包含明文密码",
        WS_TABSTOP | BS_AUTOCHECKBOX,
        0,
        hwnd,
        ID_EXPORT_PASSWORDS,
        (36, 126, 260, 24),
    );
    s.ack = ctl(
        "BUTTON",
        "我明白导出文件是明文,会妥善保管",
        WS_TABSTOP | BS_AUTOCHECKBOX,
        0,
        hwnd,
        ID_EXPORT_ACK,
        (36, 156, 270, 24),
    );
    ctl(
        "STATIC",
        "CSV:7 列,Excel 可直接打开,不含时间戳。\r\nJSON:完整备份,保留分类、标签与时间。",
        SS_LEFT,
        0,
        hwnd,
        ID_EXPORT_NOTE,
        (36, 190, 270, 60),
    );
    ctl("BUTTON", "导出到文件…", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_EXPORT_BTN, (36, 258, 160, 38));
    ctl(
        "STATIC",
        "回收站中的条目不会被导出。",
        SS_LEFT,
        0,
        hwnd,
        ID_EXPORT_FOOTNOTE,
        (36, 310, 270, 22),
    );

    // ---- 导入 ----
    ctl("BUTTON", "导入", BS_GROUPBOX, 0, hwnd, ID_IMPORT_GROUP, (340, 62, 300, 320));
    ctl("STATIC", "重复条目", SS_LEFT, 0, hwnd, ID_IMPORT_STRATEGY_LABEL, (356, 88, 120, 22));
    s.strategy = combo(hwnd, ID_IMPORT_STRATEGY, (356, 112, 268, 200));
    ctl(
        "STATIC",
        "按「标题 + 用户名」判断是否重复(忽略大小写)。\r\n覆盖时:文件里没有密码的条目会保留原密码。",
        SS_LEFT,
        0,
        hwnd,
        ID_IMPORT_HINT,
        (356, 152, 268, 60),
    );
    ctl("BUTTON", "从文件导入…", WS_TABSTOP | BS_PUSHBUTTON, 0, hwnd, ID_IMPORT_BTN, (356, 258, 160, 38));
    ctl(
        "STATIC",
        "导入前会自动生成 data.pkk.bak 备份,\r\n并为每条记录分配新的 id。",
        SS_LEFT,
        0,
        hwnd,
        ID_IMPORT_FOOTNOTE,
        (356, 306, 268, 44),
    );

    s.status = ctl("STATIC", "", SS_LEFT, 0, hwnd, ID_STATUS, (20, 388, 620, 44));
    ctl("BUTTON", "关闭", WS_TABSTOP | BS_DEFPUSHBUTTON, 0, hwnd, ID_CLOSE, (520, 442, 120, 38));

    ui::combo_add(s.format, Format::Csv.label());
    ui::combo_add(s.format, Format::Json.label());
    ui::combo_set_index(s.format, 0);

    ui::combo_add(s.strategy, DuplicateStrategy::Skip.label());
    ui::combo_add(s.strategy, DuplicateStrategy::Overwrite.label());
    ui::combo_add(s.strategy, DuplicateStrategy::Append.label());
    ui::combo_set_index(s.strategy, 0);

    ui::set_checked(s.passwords, true);

    ui::apply_font_to(hwnd, &CONTROL_IDS, app::state().font);
}

fn on_command(hwnd: HWND, id: usize, code: u16) {
    if code != BN_CLICKED {
        return;
    }

    match id {
        // 不导出密码时,那句确认就没有对象:一并禁用并清掉勾选,
        // 免得「先勾确认、再取消密码」留下一个无意义的已确认状态。
        ID_EXPORT_PASSWORDS => {
            let include = ui::is_checked(st(hwnd).passwords);
            ui::enable(st(hwnd).ack, include);
            if !include {
                ui::set_checked(st(hwnd).ack, false);
            }
            ui::set_text(st(hwnd).status, "");
        }
        ID_EXPORT_BTN => export_to_file(hwnd),
        ID_IMPORT_BTN => import_from_file(hwnd),
        ID_CLOSE => ui::destroy_window(hwnd),
        _ => {}
    }
}

fn export_to_file(hwnd: HWND) {
    let format = if ui::combo_index(st(hwnd).format) == 1 {
        Format::Json
    } else {
        Format::Csv
    };
    let include_passwords = ui::is_checked(st(hwnd).passwords);

    if include_passwords && !ui::is_checked(st(hwnd).ack) {
        ui::set_text(
            st(hwnd).status,
            "导出明文密码前,请先勾选「我明白导出文件是明文,会妥善保管」。",
        );
        return;
    }

    if app::state().vault.entry_count() == 0 {
        ui::set_text(st(hwnd).status, "密码本里还没有条目,没有可导出的内容。");
        return;
    }

    let (label, pattern) = format.filter();
    let default_name = format!(
        "PasswordNotebook-{}.{}",
        timefmt::local_date_string(timefmt::now()),
        format.extension()
    );
    let Some(path) = ui::pick_file(
        hwnd,
        true,
        &[(label, pattern), ("所有文件 (*.*)", "*.*")],
        &default_name,
    ) else {
        return;
    };

    // `pick_file` 会弹出系统模态对话框:之后再取状态,不要跨过它复用旧引用。
    let options = ExportOptions { include_passwords };
    let result = match app::state().vault.document() {
        Some(document) => export_import::export_to_path(document, Path::new(&path), format, options),
        None => {
            ui::set_text(st(hwnd).status, "密码本尚未解锁。");
            return;
        }
    };

    match result {
        Ok(count) => {
            ui::set_text(st(hwnd).status, &format!("已导出 {count} 条记录到 {path}。"));
            let note = if include_passwords {
                "文件中的密码是明文,请尽快转移到安全位置并删除原文件。"
            } else {
                "文件中不含密码。"
            };
            ui::info(hwnd, &format!("已导出 {count} 条记录。\n\n{note}"), "导出完成");
        }
        Err(e) => {
            ui::set_text(st(hwnd).status, &e.to_string());
            ui::error(hwnd, &e.to_string(), "导出失败");
        }
    }
}

fn selected_strategy(hwnd: HWND) -> DuplicateStrategy {
    match ui::combo_index(st(hwnd).strategy) {
        1 => DuplicateStrategy::Overwrite,
        2 => DuplicateStrategy::Append,
        _ => DuplicateStrategy::Skip,
    }
}

fn import_from_file(hwnd: HWND) {
    let Some(path) = ui::pick_file(
        hwnd,
        false,
        &[
            ("CSV / JSON 文件 (*.csv;*.json)", "*.csv;*.json"),
            ("所有文件 (*.*)", "*.*"),
        ],
        "",
    ) else {
        return;
    };

    let entries = match export_import::import_from_path(Path::new(&path)) {
        Ok(entries) => entries,
        Err(e) => {
            ui::set_text(st(hwnd).status, &e.to_string());
            ui::error(hwnd, &e.to_string(), "无法读取该文件");
            return;
        }
    };

    let strategy = selected_strategy(hwnd);
    let current = app::state().vault.entry_count();
    let question = format!(
        "将导入 {} 条记录。\n\n当前密码本里有 {current} 条,重复条目的处理方式:{}。\n\n导入前会把库文件备份为 data.pkk.bak。是否继续?",
        entries.len(),
        strategy.label()
    );
    if !ui::confirm(hwnd, &question, "确认导入") {
        return;
    }

    // 先备份:备份失败就不往下走,免得用户以为还能回滚。
    let backup = match app::state().vault.backup_file() {
        Ok(path) => path,
        Err(e) => {
            let message = format!("无法生成备份({e}),导入已取消,原文件未改动。");
            ui::set_text(st(hwnd).status, &message);
            ui::error(hwnd, &message, "备份失败");
            return;
        }
    };

    match app::state().vault.import_entries(entries, strategy) {
        Ok(outcome) => {
            if outcome.changed() {
                st(hwnd).changed = true;
            }
            let summary = format!(
                "导入完成:{},原文件已备份为 {}。",
                outcome.summary(),
                backup.display()
            );
            ui::set_text(st(hwnd).status, &summary);
            ui::info(hwnd, &summary, "导入完成");
        }
        Err(e) => {
            // 导入是「写盘成功才提交内存」的:失败时磁盘上的库文件一个字都没变,
            // 所以这里**不能**再教用户去改 .bak 覆盖 —— 程序还在运行时照着做,
            // 会被下一次保存用内存态整份盖掉,反而更危险。
            let message = format!(
                "{e}\n\n磁盘上的库文件没有被改动,不需要做任何回滚。\n导入前的备份仍保留在:{}",
                backup.display()
            );
            ui::set_text(st(hwnd).status, &e.to_string());
            ui::error(hwnd, &message, "导入失败");
        }
    }
}
