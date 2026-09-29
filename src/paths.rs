//! 路径与设置读写。
//!
//! 设计:**只有一个数据库,文件名固定**,放在程序所在目录。
//! 打开程序时只要判断这个文件在不在 —— 在就进入解锁流程,不在就走初始化创建。
//! 程序目录不可写时(例如装进 Program Files)回退到「我的文档」。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 数据库文件扩展名。
pub const VAULT_EXTENSION: &str = "pkk";

/// 数据库文件名(固定,避免出现多个库)。
pub const VAULT_FILE_NAME: &str = "data.pkk";

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn documents_dir() -> PathBuf {
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return PathBuf::from(profile).join("Documents");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join("Documents");
    }
    PathBuf::from(".")
}

fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".pkk-write-probe");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// 数据库与设置文件存放目录(结果只探测一次并缓存)。
pub fn data_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let exe = exe_dir();
        if is_writable(&exe) {
            exe
        } else {
            documents_dir()
        }
    })
    .as_path()
}

/// 数据库文件的唯一路径。
pub fn vault_path() -> PathBuf {
    data_dir().join(VAULT_FILE_NAME)
}

/// 界面上显示的数据库路径。
///
/// 正常情况下只显示**文件名**:界面上出现 `C:\Users\...` 这种绝对路径,
/// 会让人以为路径是写死的。只有真的回退到程序目录之外时才显示完整路径 ——
/// 那种情况你需要知道文件到底在哪。
pub fn vault_display_path() -> String {
    if data_dir() == exe_dir() {
        VAULT_FILE_NAME.to_string()
    } else {
        vault_path().to_string_lossy().into_owned()
    }
}

/// 数据库是否已存在。
pub fn vault_exists() -> bool {
    vault_path().is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_is_writable() {
        assert!(is_writable(data_dir()));
    }

    #[test]
    fn display_path_hides_directory_in_normal_case() {
        // 测试环境里程序目录可写,所以只显示文件名。
        assert_eq!(vault_display_path(), VAULT_FILE_NAME);
    }

    #[test]
    fn vault_path_is_fixed_and_inside_data_dir() {
        let path = vault_path();
        assert!(path.starts_with(data_dir()));
        assert_eq!(path.file_name().unwrap(), VAULT_FILE_NAME);
        assert_eq!(path.extension().unwrap(), VAULT_EXTENSION);
        // 固定文件名:重复调用结果一致。
        assert_eq!(path, vault_path());
    }
}
