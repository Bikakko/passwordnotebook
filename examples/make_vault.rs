//! 开发用小工具:创建一个测试密码本。
//!
//! ```text
//! cargo run --example make_vault -- <路径> <主密码>
//! ```
//!
//! 因为加解密逻辑是可移植的,这个工具在 Linux 上跑出来的 `.pkk`,
//! Windows 端的程序可以直接打开 —— 也顺便验证了格式的跨平台一致性。

use std::path::Path;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(path), Some(password)) = (args.next(), args.next()) else {
        eprintln!("用法:make_vault <路径> <主密码>");
        std::process::exit(2);
    };

    match password_notebook::vault::VaultService::create_new(Path::new(&path), &password) {
        Ok(code) => {
            println!("已创建:{path}");
            println!("恢复码:{code}");
        }
        Err(e) => {
            eprintln!("创建失败:{e}");
            std::process::exit(1);
        }
    }
}
