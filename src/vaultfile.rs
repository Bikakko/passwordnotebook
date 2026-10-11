//! 保险库文件读写。
//!
//! 磁盘布局:
//! ```text
//! [文件头 105B][主密码槽 48B][恢复码槽 48B][载荷长度 4B LE][载荷 ...]
//! ```

use std::io::Write;
use std::path::Path;

use crate::crypto::{self, WRAPPED_KEY_LEN};
use crate::error::VaultError;
use crate::header::{PREFIX_LEN, Reader, VaultHeader};

#[derive(Clone, Debug)]
pub struct VaultFile {
    pub header: VaultHeader,
    pub password_wrapped: Vec<u8>,
    pub recovery_wrapped: Vec<u8>,
    pub payload: Vec<u8>,
}

const FIXED_LEN: usize = PREFIX_LEN + WRAPPED_KEY_LEN * 2 + 4;

impl VaultFile {
    pub fn looks_like_vault(path: &Path) -> bool {
        let Ok(mut file) = std::fs::File::open(path) else {
            return false;
        };
        let Ok(metadata) = file.metadata() else {
            return false;
        };
        if metadata.len() < FIXED_LEN as u64 {
            return false;
        }
        let mut magic = [0u8; 4];
        std::io::Read::read_exact(&mut file, &mut magic).is_ok() && magic == crate::header::MAGIC
    }

    /// 只读文件头(固定的 105 字节前缀),不读载荷 ——
    /// 解锁前判断免密缓存是否可用的场景用不到后面那几 MB。
    pub fn read_header(path: &Path) -> Result<VaultHeader, VaultError> {
        let mut buf = [0u8; PREFIX_LEN];
        let mut file = std::fs::File::open(path)?;
        read_exact_or_truncated(&mut file, &mut buf)?;
        VaultHeader::from_prefix(&buf)
    }

    /// 只读文件头与主密码槽(105 + 48 字节),不读载荷 ——
    /// 「校验一个主密码是否仍然有效」的场景用不到后面那几 MB。
    pub fn read_header_and_password_slot(
        path: &Path,
    ) -> Result<(VaultHeader, [u8; WRAPPED_KEY_LEN]), VaultError> {
        let mut buf = [0u8; PREFIX_LEN + WRAPPED_KEY_LEN];
        let mut file = std::fs::File::open(path)?;
        read_exact_or_truncated(&mut file, &mut buf)?;

        let header = VaultHeader::from_prefix(&buf[..PREFIX_LEN])?;
        let mut slot = [0u8; WRAPPED_KEY_LEN];
        slot.copy_from_slice(&buf[PREFIX_LEN..]);
        Ok((header, slot))
    }

    pub fn read(path: &Path) -> Result<Self, VaultError> {
        let bytes = std::fs::read(path)?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, VaultError> {
        if bytes.len() < FIXED_LEN {
            return Err(VaultError::Format("文件已损坏或被截断。".into()));
        }

        let header = VaultHeader::from_prefix(bytes)?;
        let mut r = Reader::new(&bytes[PREFIX_LEN..]);

        let password_wrapped = r.bytes(WRAPPED_KEY_LEN)?;
        let recovery_wrapped = r.bytes(WRAPPED_KEY_LEN)?;
        let payload_len = r.i32()?;

        if payload_len < crypto::TAG_LEN as i32 {
            return Err(VaultError::Format("文件已损坏（载荷长度无效）。".into()));
        }
        if r.remaining() < payload_len as usize {
            return Err(VaultError::Format("文件已损坏或被截断。".into()));
        }

        let payload = r.bytes(payload_len as usize)?;

        Ok(Self {
            header,
            password_wrapped,
            recovery_wrapped,
            payload,
        })
    }

    /// 原子写入:先写临时文件,再改名覆盖,避免中途失败损坏原文件。
    ///
    /// 关键是**改名之前把数据真正刷到盘上**(`sync_all`)。少了这一步,
    /// 断电后 NTFS 可能重放了改名、却没写数据 —— 此时文件还在,内容却是
    /// 全零或残缺,AEAD 校验必然失败,老内容也已被顶掉,等于库丢了。
    pub fn write_atomic(&self, path: &Path) -> Result<(), VaultError> {
        let mut out = Vec::with_capacity(FIXED_LEN + self.payload.len());
        out.extend_from_slice(&self.header.to_prefix());
        out.extend_from_slice(&self.password_wrapped);
        out.extend_from_slice(&self.recovery_wrapped);
        out.extend_from_slice(&(self.payload.len() as i32).to_le_bytes());
        out.extend_from_slice(&self.payload);

        write_atomic_bytes(path, &out)?;
        Ok(())
    }
}

/// 读满一段固定长度的前缀 / 槽位:文件不够长时报「损坏」而不是底层 IO 错误 ——
/// `UnexpectedEof` 直接透出去,界面上会显示成「文件操作失败（failed to fill
/// whole buffer）」,把「文件被截断」误装成系统 IO 故障。
fn read_exact_or_truncated(file: &mut std::fs::File, buf: &mut [u8]) -> Result<(), VaultError> {
    std::io::Read::read_exact(file, buf).map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            VaultError::Format("文件已损坏或被截断。".into())
        } else {
            VaultError::from(e)
        }
    })
}

/// 原子写一段字节:同目录临时文件 → `sync_all` → 改名顶替;失败清理临时文件。
///
/// 保险库落盘、免密缓存与明文导出共用这一套写法;临时文件按「原路径 + .tmp」
/// 命名(与各处的写盘失败注入测试约定一致)。`sync_all` 的顺序不能省,理由见
/// [`VaultFile::write_atomic`]。
pub(crate) fn write_atomic_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp_name = path.as_os_str().to_os_string();
    tmp_name.push(".tmp");
    let tmp = Path::new(&tmp_name);

    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(tmp)?;
        file.write_all(bytes)?;
        // 先落盘,再改名 —— 顺序不能反。
        file.sync_all()?;
        drop(file);
        replace_file(tmp, path)
    })();

    if let Err(e) = result {
        // 失败就把临时文件清掉,别在数据目录里留垃圾。
        let _ = std::fs::remove_file(tmp);
        return Err(e);
    }
    Ok(())
}

/// 用改名把临时文件顶替成正式文件(两者同目录,改名是原子的)。
///
/// Windows 上没有可用的「目录 fsync」,改用 `MOVEFILE_WRITE_THROUGH`
/// 让改名本身尽快落盘;POSIX 则是改名之后再 sync 一次父目录。
#[cfg(windows)]
pub(crate) fn replace_file(tmp: &Path, path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let wide = |p: &Path| -> Vec<u16> {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let from = wide(tmp);
    let to = wide(path);

    match unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } {
        Ok(()) => Ok(()),
        Err(e) => Err(std::io::Error::other(e.to_string())),
    }
}

#[cfg(not(windows))]
pub(crate) fn replace_file(tmp: &Path, path: &Path) -> std::io::Result<()> {
    std::fs::rename(tmp, path)?;

    // POSIX:目录项本身也要落盘,否则断电后可能退回旧目录项。
    #[cfg(unix)]
    if let Some(dir) = path.parent()
        && let Ok(handle) = std::fs::File::open(dir)
    {
        let _ = handle.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::WRAPPED_KEY_LEN;

    fn sample() -> VaultFile {
        VaultFile {
            header: VaultHeader::generate().unwrap(),
            password_wrapped: vec![1u8; WRAPPED_KEY_LEN],
            recovery_wrapped: vec![2u8; WRAPPED_KEY_LEN],
            payload: vec![3u8; 32],
        }
    }

    #[test]
    fn roundtrip() {
        let file = sample();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&file.header.to_prefix());
        bytes.extend_from_slice(&file.password_wrapped);
        bytes.extend_from_slice(&file.recovery_wrapped);
        bytes.extend_from_slice(&(file.payload.len() as i32).to_le_bytes());
        bytes.extend_from_slice(&file.payload);

        let parsed = VaultFile::parse(&bytes).unwrap();
        assert_eq!(parsed.header.vault_id, file.header.vault_id);
        assert_eq!(parsed.password_wrapped, file.password_wrapped);
        assert_eq!(parsed.recovery_wrapped, file.recovery_wrapped);
        assert_eq!(parsed.payload, file.payload);
    }

    #[test]
    fn truncated_is_rejected() {
        let bytes = vec![0u8; 10];
        assert!(VaultFile::parse(&bytes).is_err());
    }

    /// 覆盖写:目标已存在时必须能顶掉(Windows 上走 MOVEFILE_REPLACE_EXISTING),
    /// 且不能留下临时文件。
    #[test]
    fn write_atomic_replaces_existing() {
        let dir = std::env::temp_dir().join(format!("pnb-rs-replace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v.pkk");

        let first = sample();
        first.write_atomic(&path).unwrap();

        let mut second = sample();
        second.payload = vec![9u8; 40];
        second.write_atomic(&path).unwrap();

        let read = VaultFile::read(&path).unwrap();
        assert_eq!(read.payload, vec![9u8; 40]);

        let tmp = path.with_extension("pkk.tmp");
        assert!(!tmp.exists(), "临时文件应已被改名,不该残留");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_atomic_and_read_back() {
        let dir = std::env::temp_dir().join(format!("pnb-rs-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v.pkk");

        let file = sample();
        file.write_atomic(&path).unwrap();
        assert!(VaultFile::looks_like_vault(&path));

        let read = VaultFile::read(&path).unwrap();
        assert_eq!(read.payload, file.payload);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 只读文件头必须与完整读取得到的头部一致;前缀被截断时必须报错。
    #[test]
    fn read_header_matches_full_read() {
        let dir = std::env::temp_dir().join(format!("pnb-rs-hdr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v.pkk");

        let file = sample();
        file.write_atomic(&path).unwrap();

        let header = VaultFile::read_header(&path).unwrap();
        let full = VaultFile::read(&path).unwrap().header;
        assert_eq!(header.vault_id, full.vault_id);
        assert_eq!(header.password_salt, full.password_salt);
        assert_eq!(header.key_generation, full.key_generation);

        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..PREFIX_LEN - 1]).unwrap();
        assert!(matches!(
            VaultFile::read_header(&path),
            Err(VaultError::Format(_))
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 主密码槽的单读必须与完整读取一致;槽都没读满(文件被截断)必须报错。
    #[test]
    fn read_password_slot_matches_full_read() {
        let dir = std::env::temp_dir().join(format!("pnb-rs-slot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v.pkk");

        let file = sample();
        file.write_atomic(&path).unwrap();

        let (header, slot) = VaultFile::read_header_and_password_slot(&path).unwrap();
        assert_eq!(header.password_salt, file.header.password_salt);
        assert_eq!(slot.as_slice(), file.password_wrapped.as_slice());

        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..PREFIX_LEN + WRAPPED_KEY_LEN - 1]).unwrap();
        assert!(matches!(
            VaultFile::read_header_and_password_slot(&path),
            Err(VaultError::Format(_))
        ));

        std::fs::remove_dir_all(&dir).ok();
    }
}
