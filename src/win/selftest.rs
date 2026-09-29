//! 自检:验证核心加密逻辑与窗口退出路径。
//!
//! 用法:`PasswordNotebook.exe --selftest`
//! 其中「窗口生命周期」一项专门用来验收「关闭后进程彻底退出」:
//! 创建真实主窗口 → 定时自动关闭 → 消息循环退出 → 进程结束。

use crate::model::Entry;
use crate::vault::VaultService;
use crate::vaultfile::VaultFile;

pub fn requested() -> bool {
    std::env::args().any(|a| a == "--selftest")
}

pub fn run() -> i32 {
    // GUI 子系统下没有控制台,先把父进程的控制台接过来,后续 println! 才有地方输出。
    super::attach_parent_console();

    println!("=== Password Notebook 自检 ===");

    println!("-- 路径(全部由 exe 位置推导,无硬编码)--");
    println!("  程序所在目录:{}", crate::paths::exe_dir().display());
    println!("  数据目录    :{}", crate::paths::data_dir().display());
    println!("  数据库文件  :{}", crate::paths::vault_path().display());
    println!(
        "  数据库已存在:{}",
        if crate::paths::vault_exists() { "是" } else { "否" }
    );

    println!("-- 高 DPI 诊断 --");
    println!(
        "  GetDpiForSystem = {}",
        unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() }
    );
    let (raw_font, scaled_font) = super::ui::message_font_height();
    println!("  消息字体 lfHeight:原始 {raw_font} → 缩放后 {scaled_font}");
    println!(
        "  屏幕尺寸 = {}x{}",
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN,
            )
        },
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN,
            )
        }
    );

    let mut failures = 0usize;

    println!("-- 核心逻辑 --");
    failures += core_checks();

    println!("-- 通用控件版本(决定外观是否现代化)--");
    match super::ui::module_path("comctl32.dll") {
        Some(path) => {
            let modern = path.to_lowercase().contains("winsxs");
            println!("  comctl32.dll = {path}");
            if modern {
                println!("  PASS  已启用 comctl32 v6(现代主题外观)");
            } else {
                println!("  FAIL  仍是 v5(Windows 95 风格),manifest 未生效");
                failures += 1;
            }
        }
        None => println!("  SKIP  未能取得 comctl32 路径"),
    }

    println!("-- 标签页控件 --");
    failures += tabs_probe();

    println!("-- 窗口生命周期 --");

    // 回归:Windows Hello 按钮只应在解锁界面出现。
    // 做法是在另一个线程里用控件 id 去问窗口(只读查询,跨线程安全),
    // 模式切换通过向主窗口投递「点击新建密码本」完成 —— 走真实代码路径。
    let observed = std::sync::Arc::new((
        std::sync::atomic::AtomicI32::new(-1),
        std::sync::atomic::AtomicI32::new(-1),
    ));
    let watcher = {
        let observed = std::sync::Arc::clone(&observed);
        std::thread::spawn(move || {
            use std::sync::atomic::Ordering;
            use windows::core::PCWSTR;
            use windows::Win32::Foundation::{LPARAM, WPARAM};
            use windows::Win32::UI::WindowsAndMessaging::{
                FindWindowW, GetDlgItem, IsWindowVisible, PostMessageW, ShowWindow, SW_SHOW,
                WM_COMMAND,
            };

            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(20));
                let class = super::ui::Wz::new(super::main_window::CLASS_NAME);
                let Ok(main) = (unsafe { FindWindowW(class.pcwstr(), PCWSTR::null()) }) else {
                    continue;
                };
                let Ok(hello) =
                    (unsafe { GetDlgItem(Some(main), super::main_ui::ID_HELLO_BTN as i32) })
                else {
                    continue;
                };
                if main.is_invalid() || hello.is_invalid() {
                    continue;
                }

                // 强制设为可见:模拟「解锁界面里这个按钮本来就显示着」
                // (真实机器上有免密缓存时就是这样)。修复若被撤掉,
                // 切换形态后它不会消失,本断言就会失败。
                unsafe {
                    let _ = ShowWindow(hello, SW_SHOW);
                }
                let in_unlock = unsafe { IsWindowVisible(hello) }.as_bool();
                // 等价于点「新建密码本」。用 PostMessage(异步)避免阻塞观测线程。
                unsafe {
                    let _ = PostMessageW(
                        Some(main),
                        WM_COMMAND,
                        WPARAM(super::main_ui::ID_GOTO_CREATE_BTN),
                        LPARAM(0),
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(80));
                let in_create = unsafe { IsWindowVisible(hello) }.as_bool();

                observed.0.store(i32::from(in_unlock), Ordering::SeqCst);
                observed.1.store(i32::from(in_create), Ordering::SeqCst);
                return;
            }
        })
    };

    let code = super::main_window::run_main_inner(Some(900));
    let _ = watcher.join();

    {
        use std::sync::atomic::Ordering;
        let in_unlock = observed.0.load(Ordering::SeqCst);
        let in_create = observed.1.load(Ordering::SeqCst);
        if in_unlock < 0 || in_create < 0 {
            println!("  SKIP  未观测到主窗口(当前不是解锁形态?)");
        } else {
            let shown = |v: i32| if v == 1 { "显示" } else { "隐藏" };
            println!(
                "  Hello 按钮:解锁界面={} → 创建界面={}",
                shown(in_unlock),
                shown(in_create)
            );
            if in_create == 1 {
                println!("  FAIL  切到别的形态后 Hello 按钮仍然可见");
                failures += 1;
            } else if in_unlock == 1 {
                println!("  PASS  切换形态后 Hello 按钮已隐藏");
            } else {
                println!("  PASS  Hello 按钮两种形态下都不可见(本次未走到显示路径)");
            }
        }
    }
    if code == 0 {
        println!("  PASS  创建窗口 → 自动关闭 → 消息循环正常退出");
    } else {
        println!("  FAIL  消息循环退出码 {code}");
        failures += 1;
    }

    println!();
    if failures == 0 {
        println!("结果:全部通过");
        0
    } else {
        println!("结果:{failures} 项失败");
        1
    }
}

fn core_checks() -> usize {
    let mut failures = 0usize;

    let dir = std::env::temp_dir().join(format!("pnb-selftest-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("vault.pkk");
    let _ = std::fs::remove_file(&path);

    macro_rules! check {
        ($name:expr, $cond:expr) => {
            if $cond {
                println!("  PASS  {}", $name);
            } else {
                println!("  FAIL  {}", $name);
                failures += 1;
            }
        };
    }

    const M: u32 = 8;
    const T: u32 = 1;
    const P: u32 = 1;
    const PWD: &str = "Self-Test-Password-1!";
    const NEW_PWD: &str = "Self-Test-Password-2!";

    let recovery = match VaultService::create_new_with_params(&path, PWD, M, T, P) {
        Ok(code) => {
            check!("创建密码本并返回恢复码", code.replace('-', "").len() == 32);
            code
        }
        Err(e) => {
            println!("  FAIL  创建密码本:{e}");
            failures += 1;
            String::new()
        }
    };

    check!("文件可被识别为密码本", VaultFile::looks_like_vault(&path));
    check!("正确登录密码可解锁", VaultService::new().open(&path, PWD).is_ok());
    check!(
        "错误登录密码被拒绝",
        VaultService::new().open(&path, "wrong-password").is_err()
    );

    let mut unlocked = VaultService::new();
    if unlocked.open(&path, PWD).is_ok() {
        let mut item = Entry {
            title: "GitHub".into(),
            username: "alice".into(),
            password: zeroize::Zeroizing::new("s3cret".to_string()),
            category: "开发".into(),
            tags: vec!["工作".into()],
            ..Default::default()
        };
        item.id = Entry::new_id().unwrap_or_default();

        check!("新增条目", unlocked.add_entry(item).is_ok());
        check!("条目已落盘并可重读", unlocked.entry_count() == 1);

        let id = unlocked
            .active_entries()
            .next()
            .map(|e| e.id.clone())
            .unwrap_or_default();
        check!("移入回收站", unlocked.move_to_bin(&id).is_ok());
        check!("回收站中有一条", unlocked.deleted_count() == 1);
        check!("从回收站恢复", unlocked.restore_from_bin(&id).is_ok());
        check!("彻底删除", unlocked.purge(&id).is_ok());

        check!(
            "修改登录密码",
            unlocked.change_master_password(PWD, NEW_PWD).is_ok()
        );

        // 设置写在库里,不产生额外的配置文件。
        let mut settings = crate::model::Settings::default();
        settings.clipboard_clear_seconds = 42;
        settings.idle_lock_minutes = 7;
        check!("设置写入库内", unlocked.update_settings(settings).is_ok());
    }

    check!("新登录密码可用", VaultService::new().open(&path, NEW_PWD).is_ok());

    {
        let mut reopened = VaultService::new();
        let ok = reopened.open(&path, NEW_PWD).is_ok();
        let persisted = reopened
            .document()
            .map(|d| (d.settings.clipboard_clear_seconds, d.settings.idle_lock_minutes));
        check!(
            "设置随库持久化(42 / 7 分钟)",
            ok && persisted == Some((42, 7))
        );
    }
    check!(
        "旧登录密码已失效",
        VaultService::new().open(&path, PWD).is_err()
    );

    if recovery.is_empty() {
        check!("恢复码可解锁", false);
    } else {
        check!(
            "改密后恢复码仍可解锁",
            VaultService::new()
                .open_with_recovery_code(&path, &recovery)
                .is_ok()
        );
    }

    // 篡改检测
    if let Ok(mut bytes) = std::fs::read(&path) {
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        let tampered = dir.join("tampered.pkk");
        let _ = std::fs::write(&tampered, &bytes);
        check!(
            "篡改载荷后无法解锁",
            VaultService::new().open(&tampered, NEW_PWD).is_err()
        );
    }

    // DPAPI 本机免密缓存
    super::dpapi::clear();
    let vault_id = [9u8; 16];
    super::dpapi::store(vault_id, 3, &[7u8; 32], true);
    let loaded = super::dpapi::load(vault_id, 3);
    check!(
        "DPAPI 缓存可写入并读回",
        loaded
            .as_ref()
            .is_some_and(|(d, h, _)| d[..] == [7u8; 32][..] && *h)
    );
    check!(
        "KeyGeneration 不匹配时缓存失效",
        super::dpapi::load(vault_id, 4).is_none()
    );
    super::dpapi::clear();
    check!("清除后缓存不存在", !super::dpapi::exists());

    // 回归:历史上长度恰为 25..=28 字节的残缺缓存会越过长度校验、越界 panic。
    // 构造「头匹配但长度字段缺失」的文件,必须优雅拒绝。
    if let Some(p) = super::dpapi::cache_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut truncated_rejected = true;
        for len in 25usize..=28 {
            let mut buf = Vec::new();
            buf.extend_from_slice(b"PNBQ");
            buf.extend_from_slice(&vault_id);
            buf.extend_from_slice(&3u32.to_le_bytes());
            buf.resize(len, 0);
            let _ = std::fs::write(&p, &buf);
            if super::dpapi::load(vault_id, 3).is_some() {
                truncated_rejected = false;
            }
        }
        check!("残缺缓存(25..=28B)被拒绝而非 panic", truncated_rejected);
        super::dpapi::clear();
    }

    // 剪贴板
    if super::clipboard::set_text("pnb-clipboard-probe") {
        check!(
            "剪贴板读写",
            super::clipboard::get_text().as_deref() == Some("pnb-clipboard-probe")
        );
    } else {
        println!("  SKIP  剪贴板(当前会话不可用)");
    }

    check!("系统空闲时间可读取", super::idle::idle_seconds() < 86_400);

    let _ = std::fs::remove_dir_all(&dir);
    failures
}


/// 无界面探针:验证自绘标签条的增删与选中逻辑。
fn tabs_probe() -> usize {
    use super::sys::*;
    use super::ui;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

    unsafe extern "system" fn probe_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    let mut failures = 0;

    {
        let _ = ui::register_class("PnbTabProbe", probe_proc);
        let host =
            ui::create_window("PnbTabProbe", "", WS_OVERLAPPED, 0, HWND::default(), 0, 0, 0, 400, 300);
        if host.is_invalid() {
            println!("  FAIL  无法创建宿主窗口");
            return 1;
        }

        ui::register_tab_strip_class();
        let tabs = ui::create_window("PnbTabStrip", "", WS_CHILD, 0, host, 1, 0, 0, 380, 200);

        ui::tabs_clear(tabs);
        ui::tabs_add(tabs, "全部");
        ui::tabs_add(tabs, "API KEY");
        ui::tabs_add(tabs, "个人");

        let names = ui::tabs_names();
        println!("  标签:{names:?}");
        if names == ["全部", "API KEY", "个人"] {
            println!("  PASS  标签名保存正确");
        } else {
            println!("  FAIL  标签名不对");
            failures += 1;
        }

        let first = ui::tabs_index(tabs);
        ui::tabs_set_index(tabs, 2);
        let after = ui::tabs_index(tabs);
        println!("  选中下标:初始={first} 设为2后={after}");
        if first == 0 && after == 2 {
            println!("  PASS  选中下标读写正确");
        } else {
            println!("  FAIL  选中下标不对");
            failures += 1;
        }

        ui::destroy_window(host);
    }

    failures
}


pub fn preview_requested() -> bool {
    std::env::args().any(|a| a == "--ui-preview")
}

/// 只画一条标签条,用来肉眼检查外观(不需要打开数据库)。
pub fn preview() -> i32 {
    use super::sys::*;
    use super::ui;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW, ShowWindow, PM_REMOVE,
        TranslateMessage, MSG, SW_SHOW, WS_OVERLAPPEDWINDOW,
    };

    unsafe extern "system" fn host_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    let _ = ui::register_class("PnbPreviewHost", host_proc);
    ui::register_tab_strip_class();

    let host = ui::create_window(
        "PnbPreviewHost",
        "标签条预览",
        WS_OVERLAPPEDWINDOW.0,
        0,
        HWND::default(),
        0,
        0,
        0,
        980,
        240,
    );
    let strip = ui::create_window(
        "PnbTabStrip",
        "",
        WS_CHILD | WS_VISIBLE,
        0,
        host,
        1,
        0,
        0,
        960,
        60,
    );

    ui::tabs_clear(strip);
    for name in ["全部", "API KEY", "个人", "一个很长的分类名称", "cloudflare"] {
        ui::tabs_add(strip, name);
    }
    ui::tabs_set_index(strip, 3);

    unsafe {
        let _ = ShowWindow(host, SW_SHOW);
    }

    // 跑一小段消息循环,让窗口真正画出来。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut msg = MSG::default();
    while std::time::Instant::now() < deadline {
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() } {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    unsafe {
        let _ = DestroyWindow(host);
    }
    println!("预览结束");
    0
}


pub fn preview_recovery_requested() -> bool {
    std::env::args().any(|a| a == "--ui-preview-recovery")
}

/// 只显示「恢复码」对话框,用来肉眼检查布局(不需要打开数据库)。
pub fn preview_recovery() -> i32 {
    use super::app;
    use super::sys::*;
    use super::ui;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

    unsafe extern "system" fn host_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    // 对话框会读全局状态里的字体和 DPI。这里必须用真实 DPI ——
    // 用 96 会导致控件按 100% 摆放、而对话框按实际缩放开尺寸,预览就假了。
    let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() }.max(96);
    app::set(app::AppState {
        settings: Default::default(),
        vault: crate::vault::VaultService::new(),
        font: Default::default(),
        font_bold: Default::default(),
        dpi,
        main: HWND::default(),
        mode: app::Mode::Create,
    });

    app::state().font = ui::create_ui_font(false, dpi);

    let _ = ui::register_class("PnbPreviewHost", host_proc);
    let host = ui::create_window("PnbPreviewHost", "", WS_OVERLAPPED, 0, HWND::default(), 0, 0, 0, 200, 200);

    super::dlg_recovery::show_code(host, "ABCD-EFGH-IJKL-MNOP-QRST-UVWX-YZ23-4567", true);

    ui::destroy_window(host);
    0
}
