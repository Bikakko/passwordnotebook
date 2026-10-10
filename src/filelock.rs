//! 解锁期间对库文件的占用锁。
//!
//! 动机:库文件只在解锁时读一次,之后所有写盘都是「临时文件 + 改名顶替」。
//! 在这两层之间,磁盘上的 `data.pkk` 谁也不占 —— 可以被外部程序删除、替换,
//! 也可以被资源管理器的撤销(Ctrl+Z)回滚,而程序对此无感,内存态与磁盘态
//! 静默分叉。极端操作链(重生成恢复码后文件被顶回旧版)能把库变成
//! 「新凭据打不开、旧凭据已销毁」。
//!
//! 这里的选择是**占用而不是检测**:解锁期间始终持有库文件句柄,共享模式只
//! 放行读取 —— 外部进程(包括资源管理器)对它删除、改名、覆盖写一律得到
//! 「文件正在被使用」;备份软件、杀毒扫描这类只读访问不受影响(内容是密文,
//! 可读无妨)。
//!
//! 写盘时必须短暂放锁:原子改名(`MoveFileExW` 替换目标)要求目标没有被
//! 「未共享删除」的句柄占着,**包括我们自己这把**。所以每次写盘的流程是
//! 「放锁 → 临时文件 + 改名 → 重新占锁」,窗口只有几毫秒,由
//! [`crate::vault::VaultService`] 的写盘路径统一处理。
//! 注意句柄占的是文件本身而不是路径:改名顶替之后,旧句柄指向的是被顶掉的
//! 旧文件,必须重新打开,锁才落在新文件上。

use std::path::Path;

/// 对库文件的占用:只要这个值活着,外部进程就不能写/删/改名对应文件。
///
/// Windows 上通过「打开句柄 + 只共享读取」实现;其它平台没有共享模式语义,
/// 退化为普通打开(界面层只有 Windows,其它平台只需要这个类型存在以便跑测试)。
pub struct FileLock {
    /// 句柄本体:字段不被读取,存在即占用。
    #[allow(dead_code)]
    file: std::fs::File,
}

impl FileLock {
    /// 打开并占用 `path`。文件必须已存在。
    ///
    /// 共享模式只放行 `FILE_SHARE_READ`:别人能读,不能写、删、改名。
    /// 若要让外部连读都不行(会同时挡住备份软件与同步客户端),把
    /// `share_mode` 改成 `0` 即可。
    pub fn acquire(path: &Path) -> std::io::Result<Self> {
        let file = open_shared_readonly(path)?;
        Ok(Self { file })
    }
}

#[cfg(windows)]
fn open_shared_readonly(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::FILE_SHARE_READ;

    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(path)
}

#[cfg(not(windows))]
fn open_shared_readonly(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pnb-lock-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("data.pkk")
    }

    #[test]
    fn lock_blocks_write_and_delete_but_allows_read() {
        let path = temp_path("hold");
        std::fs::write(&path, b"vault").unwrap();

        let lock = FileLock::acquire(&path).unwrap();
        // 读:放行(备份软件、以及本进程自己的读头路径都依赖它)。
        assert_eq!(std::fs::read(&path).unwrap(), b"vault");
        // 覆盖写与删除:都被「文件正在被使用」挡住。
        assert!(std::fs::write(&path, b"evil").is_err());
        assert!(std::fs::remove_file(&path).is_err());

        drop(lock);
        // 放锁后一切恢复。
        std::fs::write(&path, b"new").unwrap();
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn lock_survives_until_explicitly_dropped() {
        let path = temp_path("scope");
        std::fs::write(&path, b"vault").unwrap();

        {
            let _lock = FileLock::acquire(&path).unwrap();
            assert!(std::fs::remove_file(&path).is_err());
        }
        // 离开作用域即释放。
        std::fs::remove_file(&path).unwrap();
    }
}
