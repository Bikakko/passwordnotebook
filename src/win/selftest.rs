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

    println!("-- 窗口生命周期 --");
    let code = super::main_window::run_main_inner(Some(400));
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
            password: "s3cret".into(),
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
        loaded.as_ref().is_some_and(|(d, h, _)| d == &[7u8; 32] && *h)
    );
    check!(
        "KeyGeneration 不匹配时缓存失效",
        super::dpapi::load(vault_id, 4).is_none()
    );
    super::dpapi::clear();
    check!("清除后缓存不存在", !super::dpapi::exists());

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
