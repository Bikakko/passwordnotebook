//! 保险库文件读写。
//!
//! 磁盘布局:
//! ```text
//! [文件头 105B][主密码槽 48B][恢复码槽 48B][载荷长度 4B LE][载荷 ...]
//! ```

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
        match std::fs::read(path) {
            Ok(bytes) => bytes.len() >= FIXED_LEN && bytes[0..4] == crate::header::MAGIC,
            Err(_) => false,
        }
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
            return Err(VaultError::Format("文件已损坏(载荷长度无效)。".into()));
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

    /// 原子写入:先写临时文件,再重命名覆盖,避免中途失败损坏原文件。
    pub fn write_atomic(&self, path: &Path) -> Result<(), VaultError> {
        let mut out = Vec::with_capacity(FIXED_LEN + self.payload.len());
        out.extend_from_slice(&self.header.to_prefix());
        out.extend_from_slice(&self.password_wrapped);
        out.extend_from_slice(&self.recovery_wrapped);
        out.extend_from_slice(&(self.payload.len() as i32).to_le_bytes());
        out.extend_from_slice(&self.payload);

        let tmp = path.with_extension(format!("{}.tmp", crate::paths::VAULT_EXTENSION));
        std::fs::write(&tmp, &out)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
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
}
