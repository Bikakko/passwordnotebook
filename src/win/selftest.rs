//! 自检:验证核心加密逻辑与窗口退出路径。
//!
//! 用法:`pnb.exe --selftest`
//! 其中「窗口生命周期」一项专门用来验收「关闭后进程彻底退出」:
//! 创建真实主窗口 → 定时自动关闭 → 消息循环退出 → 进程结束。
//!
//! 输出会同时写进程序目录下的 `selftest-report.txt`。这不是多余的:
//! GUI 子系统程序在 PowerShell 里启动时,提示符不会等待,控制台也可能根本接不上,
//! 只靠终端输出很容易变成「一闪就没了」,报告文件是唯一稳定的验收凭据。

use std::io::Write;
use std::sync::Mutex;

use zeroize::Zeroizing;

use crate::model::Entry;
use crate::vault::VaultService;
use crate::vaultfile::VaultFile;

/// 本次自检的完整输出(除了打印,还留一份好落盘)。
static REPORT: Mutex<String> = Mutex::new(String::new());

/// 报告文件名,放在数据库所在目录(通常是 exe 同目录)。
const REPORT_FILE: &str = "selftest-report.txt";

/// 记录一行输出:先进报告缓冲,再尽力打印到控制台。
///
/// 打印**刻意忽略错误**:控制台句柄可能是「看着有效其实不可用」的继承句柄,
/// 用 `println!` 的话写失败会 panic,而 release 下是 `panic = "abort"` ——
/// 进程会直接消失,连已经收集好的报告都来不及落盘。
pub fn report_line(text: &str) {
    if let Ok(mut report) = REPORT.lock() {
        report.push_str(text);
        report.push_str("\r\n");
    }

    // 优先走控制台的宽字符接口:中文不受控制台代码页影响。
    // 标准输出被重定向到文件/管道时返回 false,再按 UTF-8 字节写。
    if super::write_console(text) {
        return;
    }

    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.write_all(b"\r\n");
    let _ = stdout.flush();
}

/// 本模块内的 `println!` 全部改走 [`report_line`],于是自动带上了报告副本。
///
/// 用同名宏覆盖标准库的 `println!` 是刻意的:自检里有上百处输出,
/// 逐个改写既啰嗦又容易漏。作用域只限本模块。
macro_rules! println {
    () => { $crate::win::selftest::report_line("") };
    ($($arg:tt)*) => { $crate::win::selftest::report_line(&::std::format!($($arg)*)) };
}

/// 把报告写到程序目录旁边,返回路径。
fn write_report() -> Option<std::path::PathBuf> {
    let path = crate::paths::data_dir().join(REPORT_FILE);
    let text = REPORT.lock().ok()?.clone();
    std::fs::write(&path, text).ok()?;
    Some(path)
}

pub fn requested() -> bool {
    std::env::args().any(|a| a == "--selftest")
}

pub fn run() -> i32 {
    // GUI 子系统下没有控制台,先把输出接回终端,后续 println! 才有地方去。
    let console = super::prepare_console();

    println!("=== Password Notebook 自检 ===");
    println!("(完整报告同时写入程序目录下的 {REPORT_FILE})");

    println!("-- 路径(全部由 exe 位置推导,无硬编码)--");
    println!("  程序所在目录:{}", crate::paths::exe_dir().display());
    println!("  数据目录    :{}", crate::paths::data_dir().display());
    println!("  数据库文件  :{}", crate::paths::vault_path().display());
    println!(
        "  数据库已存在:{}",
        if crate::paths::vault_exists() { "是" } else { "否" }
    );

    println!("-- 运行环境(环境变量异常会让下面的检查整片失败)--");
    println!("  当前目录 = {}", std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default());
    for name in ["USERNAME", "TEMP", "TMP", "LOCALAPPDATA", "USERPROFILE"] {
        println!(
            "  {name:<12} = {}",
            std::env::var(name).unwrap_or_else(|_| "(未设置)".to_string())
        );
    }
    println!("  temp_dir() = {}", std::env::temp_dir().display());

    // 直接往三个关键目录各写一次。免密缓存与临时文件都要落在这些地方,
    // 「写不进去」是最容易被误判成代码 bug 的一类环境问题。
    write_probe("程序目录", crate::paths::data_dir());
    write_probe("TEMP", &std::env::temp_dir());
    if let Some(path) = super::dpapi::cache_path()
        && let Some(dir) = path.parent()
    {
        write_probe("免密缓存目录", dir);
    }

    println!("-- 控制台诊断(中文乱码时看这里)--");
    match super::console_info() {
        Some(info) => {
            println!(
                "  输出代码页 = {} ({})",
                info.output_code_page,
                if info.output_code_page == 65001 {
                    "UTF-8"
                } else {
                    "非 UTF-8;本程序走宽字符接口,不受影响"
                }
            );
            println!(
                "  窗口字体   = {} {}px, family={:#06x}",
                info.font_face, info.font_size_y, info.font_family
            );
        }
        None => println!("  SKIP  标准输出不是控制台(可能被重定向或没有控制台)"),
    }

    println!("-- 高 DPI 诊断 --");    println!(
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

    // 报告一定要落到文件里:终端输出在 PowerShell 下可能被提示符冲掉。
    let summary = if failures == 0 {
        "自检结果:全部通过".to_string()
    } else {
        format!("自检结果:{failures} 项失败")
    };
    println!();
    println!("{summary}");

    let report_path = write_report();
    let location = match &report_path {
        Some(path) => path.display().to_string(),
        None => "(报告文件写入失败)".to_string(),
    };
    println!("完整报告:{location}");

    // 没有可用的控制台时(双击运行、或被没有终端的宿主拉起),
    // 上面这些输出用户一个也看不到 —— 弹框告诉他结果与报告位置。
    // 从终端运行时绝不弹框,免得打断脚本。
    if console != super::ConsoleState::Ready {
        let text = format!(
            "{summary}\n\n完整报告已保存到:\n{location}\n\n(从终端运行 `pnb.exe --selftest` 可直接看到全部输出)"
        );
        super::ui::info(
            windows::Win32::Foundation::HWND::default(),
            &text,
            "PasswordNotebook 自检",
        );
    }

    // 自己开的控制台窗口会随进程一起关闭,不等一下的话又是「一闪就没了」。
    if console == super::ConsoleState::Allocated {
        println!();
        println!("按回车键关闭此窗口…");
        let _ = std::io::stdin().read_line(&mut String::new());
    }

    if failures == 0 { 0 } else { 1 }
}

/// 找一个**确实可写**的自检临时目录。
///
/// 不能直接信 `std::env::temp_dir()`:从 WSL 或某些宿主启动时,`TEMP`/`TMP`
/// 可能是 Unix 风格路径或 UNC 路径,`create_dir_all` 直接失败,于是后面几十项
/// 检查全部跟着「失败」,真正的结论被淹掉(第一次真机自检就踩了这个坑)。
///
/// 因此优先用程序目录下的子目录 —— 它由 exe 位置推导,并且
/// [`crate::paths::data_dir`] 已经探测过可写性;`%TEMP%` 只作为备选。
fn scratch_dir() -> Option<std::path::PathBuf> {
    let candidates = [
        crate::paths::data_dir().join("selftest-tmp"),
        std::env::temp_dir().join(format!("pnb-selftest-{}", std::process::id())),
    ];

    for dir in candidates {
        if std::fs::create_dir_all(&dir).is_err() {
            continue;
        }
        // 目录存在不等于可写:真的写一个文件试一下。
        let probe = dir.join("write-probe.tmp");
        if std::fs::write(&probe, b"ok").is_err() {
            continue;
        }
        let _ = std::fs::remove_file(&probe);
        return Some(dir);
    }
    None
}

/// 往某个目录写一个探针文件,把结果原样打出来。
///
/// 「写不进去」这类环境问题,**测量**永远比猜测快:真机自检里
/// 「免密缓存写不进去」查了好几轮,靠的就是这一行输出。
fn write_probe(name: &str, dir: &std::path::Path) {
    let probe = dir.join("pnb-write-probe.tmp");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            println!("  {name:<12} = 可写");
        }
        Err(e) => println!("  {name:<12} = 不可写:{e}"),
    }
}

/// 免密缓存目录(`%LOCALAPPDATA%\PasswordNotebook`)是否可用。
///
/// 不可用时打印 SKIP 并让调用方跳过相关检查 —— 那种失败与代码无关,
/// 报成 FAIL 只会误导。
fn dpapi_dir_usable() -> bool {
    let Some(path) = super::dpapi::cache_path() else {
        println!("  SKIP  取不到 %LOCALAPPDATA%,跳过免密缓存检查");
        return false;
    };
    let Some(dir) = path.parent() else {
        println!("  SKIP  免密缓存路径异常,跳过检查");
        return false;
    };
    if let Err(e) = std::fs::create_dir_all(dir) {
        println!("  SKIP  免密缓存目录不可用:{} ({e})", dir.display());
        return false;
    }
    true
}

fn core_checks() -> usize {
    let mut failures = 0usize;

    let Some(dir) = scratch_dir() else {
        println!("  FAIL  找不到可写的临时目录,后续检查无法进行");
        println!(
            "        TEMP={:?} TMP={:?}",
            std::env::var("TEMP"),
            std::env::var("TMP")
        );
        return 1;
    };
    println!("  临时目录 = {}", dir.display());

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
            Zeroizing::new(String::new())
        }
    };

    check!("文件可被识别为密码本", VaultFile::looks_like_vault(&path));
    check!("正确登录密码可解锁", VaultService::new().open(&path, PWD).is_ok());
    // 注意断言的是「密码错」而不是「任意错误」:库文件不存在时也返回 Err,
    // 那种写法会在库根本没建起来的场景下谎报 PASS。
    check!(
        "错误登录密码被拒绝",
        matches!(
            VaultService::new().open(&path, "wrong-password"),
            Err(crate::error::VaultError::WrongSecret(_))
        )
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
        let settings = crate::model::Settings {
            clipboard_clear_seconds: 42,
            idle_lock_minutes: 7,
            favorites_only: true,
            column_widths: vec![147, 113, 170, 80, 160, 150],
            ..Default::default()
        };
        check!("设置写入库内", unlocked.update_settings(settings).is_ok());

        // 「未落盘」标记:挡掉下一次写盘,改动应留在内存里并被如实标记。
        check!("正常操作后没有未落盘标记", !unlocked.has_unsaved_changes());
        let blocked = path.with_extension("pkk.tmp");
        let _ = std::fs::remove_file(&blocked);
        std::fs::create_dir(&blocked).ok();
        let mut late = Entry {
            title: "未落盘".into(),
            username: "bob".into(),
            ..Default::default()
        };
        late.id = Entry::new_id().unwrap_or_default();
        check!("写盘失败时不报错地记住失败", unlocked.add_entry(late).is_err());
        check!("写盘失败后标记为未落盘", unlocked.has_unsaved_changes());
        check!("未落盘的改动仍在内存里", unlocked.entry_count() == 1);
        std::fs::remove_dir(&blocked).ok();
        check!("补一次保存即清除标记", unlocked.save().is_ok() && !unlocked.has_unsaved_changes());
        // 这条条目只是用来验证 dirty 标记的,但上面那次 save() 已经把它落盘了。
        // 不清理的话它会留在库里,让后面 transfer 段的导出/导入检查凭空多出一条
        // ——实测正是这一条让「CSV 导出带 BOM」「JSON 备份可导入」「重复导入幂等」
        // 三项一起误报 FAIL(0.2.0 没有这段自检,所以当时是通的)。
        let late_id = unlocked.active_entries().next().map(|e| e.id.clone());
        check!(
            "自检用的临时条目已清掉(不影响后续导出检查)",
            late_id.is_some_and(|id| unlocked.purge(&id).is_ok()) && unlocked.entry_count() == 0
        );
    }

    check!("新登录密码可用", VaultService::new().open(&path, NEW_PWD).is_ok());

    {
        let mut reopened = VaultService::new();
        let ok = reopened.open(&path, NEW_PWD).is_ok();
        let persisted = reopened.document().map(|d| {
            (
                d.settings.clipboard_clear_seconds,
                d.settings.idle_lock_minutes,
                d.settings.favorites_only,
                d.settings.column_widths.clone(),
            )
        });
        check!(
            "设置随库持久化(42 / 7 分钟 / 列宽 / 只看收藏)",
            ok && persisted
                == Some((
                    42,
                    7,
                    true,
                    vec![147, 113, 170, 80, 160, 150]
                ))
        );
    }
    check!(
        "旧登录密码已失效",
        matches!(
            VaultService::new().open(&path, PWD),
            Err(crate::error::VaultError::WrongSecret(_))
        )
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

    // 导入 / 导出:导出 → 重新解析 → 导入到另一个库,字段必须一一对上。
    {
        use crate::export_import::{self, DuplicateStrategy, ExportOptions, Format};

        let csv_path = dir.join("selftest-export.csv");
        let json_path = dir.join("selftest-export.json");
        let target_path = dir.join("selftest-import.pkk");
        let chrome_path = dir.join("selftest-chrome.csv");
        let gbk_path = dir.join("selftest-gbk.csv");

        let mut source = VaultService::new();
        if source.open(&path, NEW_PWD).is_ok() {
            let item = Entry {
                title: "导出用,含逗号".into(),
                username: "user".into(),
                password: zeroize::Zeroizing::new("p,w\"d".to_string()),
                api_key: zeroize::Zeroizing::new("sk-自检".to_string()),
                api_endpoint: "https://api.selftest.example".into(),
                notes: "第一行\n第二行".into(),
                category: "自检".into(),
                tags: vec!["标签A".into()],
                ..Default::default()
            };
            let _ = source.add_entry(item);

            let csv_export = source.document().map(|doc| {
                export_import::export_to_path(doc, &csv_path, Format::Csv, ExportOptions::default())
            });
            let json_export = source.document().map(|doc| {
                export_import::export_to_path(doc, &json_path, Format::Json, ExportOptions::default())
            });

            let csv_read = export_import::import_from_path(&csv_path);
            let csv_ok = matches!(&csv_read, Ok(entries)
                if entries.len() == 1
                    && entries[0].title == "导出用,含逗号"
                    && entries[0].password.as_str() == "p,w\"d"
                    && entries[0].api_key.as_str() == "sk-自检"
                    && entries[0].api_endpoint == "https://api.selftest.example");
            let csv_check_ok = csv_export.as_ref().is_some_and(|r| r.is_ok())
                && json_export.as_ref().is_some_and(|r| r.is_ok())
                && csv_ok;
            if !csv_check_ok {
                // 自检是诊断工具:失败时把真正的原因打出来,而不是只报一个 FAIL。
                // 这一步只碰文件读写,失败几乎总是「写下去的文件没能读回来」。
                if let Some(Err(e)) = &csv_export {
                    println!("        诊断:CSV 导出失败:{e}");
                }
                if let Some(Err(e)) = &json_export {
                    println!("        诊断:JSON 导出失败:{e}");
                }
                match &csv_read {
                    Ok(entries) => println!("        诊断:CSV 重新解析得到 {} 条", entries.len()),
                    Err(e) => println!("        诊断:CSV 重新解析失败:{e}"),
                }
                println!(
                    "        诊断:{} 存在 = {}",
                    csv_path.display(),
                    csv_path.exists()
                );
            }
            check!("CSV 导出带 BOM 且能被自己重新解析", csv_check_ok);
        } else {
            // 上游没建起来就别说「导出失败」——那是两回事。
            println!("  SKIP  CSV / JSON 导出(上游「打开密码本」未通过)");
        }

        let mut target = VaultService::new();
        let target_ready = VaultService::create_new_with_params(&target_path, PWD, M, T, P).is_ok()
            && target.open(&target_path, PWD).is_ok();

        if target_ready {
            let first = export_import::import_from_path(&json_path)
                .map(|entries| target.import_entries(entries, DuplicateStrategy::Skip));
            let first_ok = matches!(&first, Ok(Ok(outcome)) if outcome.added == 1);
            if !first_ok {
                println!("        诊断:JSON 首次导入结果 {first:?}");
            }
            check!("JSON 备份可导入到另一个密码本", first_ok);

            let second = export_import::import_from_path(&json_path)
                .map(|entries| target.import_entries(entries, DuplicateStrategy::Skip));
            let second_ok = matches!(&second, Ok(Ok(outcome)) if outcome.skipped == 1 && !outcome.changed());
            if !second_ok {
                println!("        诊断:JSON 重复导入结果 {second:?}");
            }
            check!("同一份备份重复导入会被跳过(幂等)", second_ok);

            check!(
                "导入前可生成 data.pkk.bak 备份",
                target.backup_file().is_ok_and(|p| p.is_file())
            );
        } else {
            println!("  SKIP  导入到新密码本(临时目录不可用或创建失败)");
        }

        let chrome_written = std::fs::write(
            &chrome_path,
            "name,url,username,password,note\n示例,https://e.example,u,p,n\n",
        )
        .is_ok();
        check!(
            "可识别 Chrome 导出的表头",
            chrome_written
                && export_import::import_from_path(&chrome_path)
                    .map(|v| v.len() == 1 && v[0].title == "示例" && v[0].url == "https://e.example")
                    .unwrap_or(false)
        );

        // 0.2.x 导出的 7 列表头(带 BOM)必须继续被认出来,否则升级后第一次
        // 导入自家旧文件,公式防护前缀就不再还原(值凭空多一个单引号)。
        let legacy_path = dir.join("selftest-legacy.csv");
        let legacy_written = std::fs::write(
            &legacy_path,
            "\u{feff}Title,Username,Password,Url,Category,Tags,Notes\r\nT,u,'=keep,https://x.example,,,\r\n",
        )
        .is_ok();
        let legacy_read = export_import::import_from_path(&legacy_path);
        let legacy_ok = legacy_written
            && matches!(&legacy_read, Ok(v) if v.len() == 1 && v[0].password.as_str() == "=keep");
        if !legacy_ok {
            println!("        诊断:旧表头文件存在={}", legacy_path.exists());
            match &legacy_read {
                Ok(v) => println!(
                    "        诊断:解析出 {} 条,首条密码前缀还原={:?}",
                    v.len(),
                    v.first().map(|e| e.password.as_str())
                ),
                Err(e) => println!("        诊断:解析失败:{e}"),
            }
        }
        check!("旧版 7 列 CSV 仍被识别为自家导出", legacy_ok);

        // 中文 Windows 上 Excel 默认另存为 GBK,必须明确报错而不是导入乱码。
        let gbk_written = std::fs::write(&gbk_path, [0xB1, 0xED, 0xCC, 0xE2, 0x0A]).is_ok();
        // 断言错误信息里点明了编码问题:只判断 is_err 的话,「文件根本不存在」
        // 也会算通过,那是个假阳性。
        let gbk_explained = export_import::import_from_path(&gbk_path)
            .err()
            .is_some_and(|e| e.to_string().contains("UTF-8"));
        check!(
            "非 UTF-8 文件被明确拒绝",
            gbk_written && gbk_explained
        );
    }

    // 篡改检测
    // 上一次自检中断留下的 0 字节文件会让 `len() - 1` 下溢 ——
    // panic=abort 下进程直接消失,连报告都看不到。
    if let Ok(mut bytes) = std::fs::read(&path)
        && let Some(last) = bytes.len().checked_sub(1)
    {
        bytes[last] ^= 0xFF;
        let tampered = dir.join("tampered.pkk");
        let _ = std::fs::write(&tampered, &bytes);
        check!(
            "篡改载荷后无法解锁",
            VaultService::new().open(&tampered, NEW_PWD).is_err()
        );
    }

    // DPAPI 本机免密缓存。目录取不到或不可写时整块跳过:那种失败与代码无关,
    // 报成 FAIL 只会把真正的问题淹掉(第一次真机自检就吃过这个亏)。
    if dpapi_dir_usable() {
        super::dpapi::clear();
        let vault_id = [9u8; 16];
        if let Err(e) = super::dpapi::store(vault_id, 3, &[7u8; 32], true) {
            println!("        store 失败:{e}");
        }
        if let Some(p) = super::dpapi::cache_path() {
            println!(
                "        缓存文件:{} 存在={} 大小={:?}",
                p.display(),
                p.exists(),
                std::fs::metadata(&p).map(|m| m.len()).ok()
            );
        }
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
            if let Some(cache_dir) = p.parent() {
                let _ = std::fs::create_dir_all(cache_dir);
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
    }

    // 剪贴板
    if super::clipboard::set_text("pnb-clipboard-probe") {
        check!(
            "剪贴板读写",
            super::clipboard::get_text().is_some_and(|t| t.as_str() == "pnb-clipboard-probe")
        );
    } else {
        println!("  SKIP  剪贴板(当前会话不可用)");
    }

    // 回归:剪贴板数据可能不带 NUL 结尾(长度由写入方决定)。
    // 整块填满非零再放上剪贴板 —— 旧代码会越过分配一路扫,新代码必须
    // 在 GlobalSize 处停下,绝不返回比实际分配更长的内容。
    {
        use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
        use windows::Win32::System::DataExchange::{
            CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
        };
        use windows::Win32::System::Memory::{
            GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
        };

        const CF_UNICODETEXT: u32 = 13;

        unsafe {
            if let Ok(handle) = GlobalAlloc(GMEM_MOVEABLE, 64) {
                let memory = HGLOBAL(handle.0);
                let size = GlobalSize(memory);
                let ptr = GlobalLock(memory) as *mut u8;

                let mut placed = false;
                if !ptr.is_null() && size >= 2 {
                    std::ptr::write_bytes(ptr, 0x41, size);
                    let _ = GlobalUnlock(memory);

                    if OpenClipboard(Some(HWND::default())).is_ok() {
                        let _ = EmptyClipboard();
                        placed = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(handle.0))).is_ok();
                        let _ = CloseClipboard();
                    }
                }

                if placed {
                    let cap = size / 2;
                    check!(
                        "非 NUL 结尾的剪贴板数据按分配大小截断",
                        super::clipboard::get_text()
                            .is_some_and(|s| s.encode_utf16().count() <= cap)
                    );
                } else {
                    let _ = GlobalFree(Some(memory));
                }
            }
        }
    }

    // 回归:模态对话框(嵌套消息循环)打开期间必须能被感知到 ——
    // 空闲自动锁定据此让路,否则编辑到一半被锁、保存必然失败、输入白填。
    {
        use super::sys::*;
        use std::sync::atomic::{AtomicBool, Ordering};
        use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

        static SAW_OPEN: AtomicBool = AtomicBool::new(false);

        unsafe extern "system" fn probe_proc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            match msg {
                WM_CREATE => {
                    super::ui::set_timer(hwnd, 1, 30);
                    LRESULT(0)
                }
                WM_TIMER => {
                    // 此刻正处于 run_modal 的嵌套循环里。
                    SAW_OPEN.store(super::dialog::is_modal_open(), Ordering::SeqCst);
                    super::ui::destroy_window(hwnd);
                    LRESULT(0)
                }
                _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
            }
        }

        check!(
            "模态打开前 dialog::is_modal_open() 为假",
            !super::dialog::is_modal_open()
        );
        let _ = super::dialog::open(
            "PnbModalProbe",
            "模态探针",
            HWND::default(),
            probe_proc,
            Box::new(()),
            220,
            120,
        );
        check!(
            "嵌套消息循环期间 dialog::is_modal_open() 为真",
            SAW_OPEN.load(Ordering::SeqCst)
        );
        check!(
            "模态关闭后 dialog::is_modal_open() 复位",
            !super::dialog::is_modal_open()
        );
    }

    // 回归:窗口状态来自注册表,showCmd 未校验时野值会让窗口启动即隐藏。
    check!(
        "showCmd 校验:0(SW_HIDE)等野值被拒",
        !super::window_state::show_cmd_is_valid(0)
            && !super::window_state::show_cmd_is_valid(6)
            && !super::window_state::show_cmd_is_valid(u32::MAX)
    );
    check!(
        "showCmd 校验:三种合法值被接受",
        super::window_state::show_cmd_is_valid(super::sys::SW_SHOWNORMAL as u32)
            && super::window_state::show_cmd_is_valid(super::sys::SW_SHOWMINIMIZED as u32)
            && super::window_state::show_cmd_is_valid(super::sys::SW_SHOWMAXIMIZED as u32)
    );

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


/// 自检预览共用的宿主窗口:`mode` 为 `Some` 时先装好全局状态与字体
/// (恢复码 / 导入导出对话框要读它们),标签条预览传 `None`。
fn preview_host(
    mode: Option<super::app::Mode>,
    style: u32,
    title: &str,
    width: i32,
    height: i32,
) -> windows::Win32::Foundation::HWND {
    use super::app;
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

    if let Some(mode) = mode {
        // 对话框会读全局状态里的字体和 DPI。这里必须用真实 DPI ——
        // 用 96 会导致控件按 100% 摆放、而对话框按实际缩放开尺寸,预览就假了。
        let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() }.max(96);
        app::set(app::AppState {
            settings: Default::default(),
            vault: crate::vault::VaultService::new(),
            font: Default::default(),
            font_bold: Default::default(),
            font_icon_lg: Default::default(),
            font_icon_sm: Default::default(),
            dpi,
            main: HWND::default(),
            mode,
        });
        app::state().font = ui::create_ui_font(false, dpi);
        app::state().font_bold = ui::create_ui_font(true, dpi);
        app::state().font_icon_lg = ui::create_icon_font(dpi, 40);
        app::state().font_icon_sm = ui::create_icon_font(dpi, 14);
    }

    let _ = ui::register_class("PnbPreviewHost", host_proc);
    ui::create_window("PnbPreviewHost", title, style, 0, HWND::default(), 0, 0, 0, width, height)
}

/// 只画一条标签条,用来肉眼检查外观(不需要打开数据库)。
pub fn preview() -> i32 {
    use super::sys::*;
    use super::ui;
    use windows::Win32::UI::WindowsAndMessaging::{
        DestroyWindow, DispatchMessageW, PeekMessageW, ShowWindow, PM_REMOVE, TranslateMessage,
        MSG, SW_SHOW, WS_OVERLAPPEDWINDOW,
    };

    let host = preview_host(None, WS_OVERLAPPEDWINDOW.0, "标签条预览", 980, 240);
    ui::register_tab_strip_class();

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


/// 只显示「恢复码」对话框,用来肉眼检查布局(不需要打开数据库)。
pub fn preview_recovery() -> i32 {
    use super::app;
    use super::sys::*;
    use super::ui;

    let host = preview_host(Some(app::Mode::Create), WS_OVERLAPPED, "", 200, 200);

    super::dlg_recovery::show_code(host, "ABCD-EFGH-IJKL-MNOP-QRST-UVWX-YZ23-4567", true);

    ui::destroy_window(host);
    0
}

/// 只显示「导入 / 导出」对话框,用来肉眼检查布局(不需要打开数据库)。
///
/// 预览时库是锁着的,点按钮只会提示未解锁 —— 这里看的是排版与文案。
pub fn preview_transfer() -> i32 {
    use super::app;
    use super::sys::*;
    use super::ui;

    let host = preview_host(Some(app::Mode::Unlocked), WS_OVERLAPPED, "", 200, 200);

    super::dlg_transfer::show(host);

    ui::destroy_window(host);
    0
}

/// 识别三个预览开关并运行对应预览;没有开关返回 `None`。
pub fn preview_mode() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--ui-preview") {
        Some(preview())
    } else if args.iter().any(|a| a == "--ui-preview-recovery") {
        Some(preview_recovery())
    } else if args.iter().any(|a| a == "--ui-preview-transfer") {
        Some(preview_transfer())
    } else {
        None
    }
}
