//! 把 manifest 作为资源编进 exe。
//!
//! 没有它,Windows 会让通用控件走 comctl32 v5(Windows 95 风格),
//! 按钮、列表边框、标签页都会是那种扁平灰的老样子。

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=app.ico");

    // 只有给 Windows 目标编译时才需要 windres。
    if std::env::var("CARGO_CFG_WINDOWS").is_err() {
        return;
    }

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let resource = format!("{out_dir}/app.res");

    let status = std::process::Command::new("x86_64-w64-mingw32-windres")
        .args(["app.rc", "-O", "coff", "-o", &resource])
        .status()
        .expect("需要 x86_64-w64-mingw32-windres(mingw-w64 的一部分)");

    assert!(status.success(), "windres 编译资源失败");
    println!("cargo:rustc-link-arg={resource}");
}
