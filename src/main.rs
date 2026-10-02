// GUI 子系统:双击运行时不弹出多余的命令行窗口。
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    std::process::exit(password_notebook::win::run());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("PasswordNotebook 是 Windows 原生程序，当前平台不受支持。");
    std::process::exit(1);
}
