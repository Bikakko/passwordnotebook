//! 保险库文件头。
//!
//! 文件头是明文,但会作为 AES-GCM 的附加认证数据(AAD),因此任何篡改都会导致解密失败。
//! 三个 AAD 相互独立(主密码槽 / 恢复码槽 / 载荷),所以改主密码不会破坏恢复码槽和载荷。

use crate::crypto::{self, NONCE_LEN, SALT_LEN};
use crate::error::VaultError;

pub const MAGIC: [u8; 4] = *b"PNBK";
pub const FORMAT_VERSION: u8 = 1;

/// 文件头固定长度:
/// magic(4) ver(1) m(4) t(4) p(4) vault_id(16) keygen(4)
/// + pw_salt(16) pw_nonce(12) rec_salt(16) rec_nonce(12) payload_nonce(12)
pub const PREFIX_LEN: usize = 105;

pub const VAULT_ID_LEN: usize = 16;

#[derive(Clone, Debug)]
pub struct VaultHeader {
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u32,
    pub vault_id: [u8; VAULT_ID_LEN],
    /// 每次修改主密码自增,用于让本机免密缓存自动失效。
    pub key_generation: u32,
    pub password_salt: [u8; SALT_LEN],
    pub password_nonce: [u8; NONCE_LEN],
    pub recovery_salt: [u8; SALT_LEN],
    pub recovery_nonce: [u8; NONCE_LEN],
    /// 载荷随机数:每次保存都换新的,因此不能用来标识文件版本。
    pub payload_nonce: [u8; NONCE_LEN],
}

impl VaultHeader {
    pub fn generate() -> Result<Self, VaultError> {
        Self::generate_with(
            crypto::DEFAULT_M_COST_KIB,
            crypto::DEFAULT_T_COST,
            crypto::DEFAULT_P_COST,
        )
    }

    /// 指定 KDF 参数生成头部(仅测试会用到非默认参数)。
    pub fn generate_with(m_cost_kib: u32, t_cost: u32, p_cost: u32) -> Result<Self, VaultError> {
        Ok(Self {
            m_cost_kib,
            t_cost,
            p_cost,
            vault_id: crypto::random_array()?,
            key_generation: 1,
            password_salt: crypto::random_array()?,
            password_nonce: crypto::random_array()?,
            recovery_salt: crypto::random_array()?,
            recovery_nonce: crypto::random_array()?,
            payload_nonce: crypto::random_array()?,
        })
    }

    /// AAD 的公共前缀:magic + version + KDF 参数 + vault_id。
    fn aad_base(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(33);
        v.extend_from_slice(&MAGIC);
        v.push(FORMAT_VERSION);
        v.extend_from_slice(&self.m_cost_kib.to_le_bytes());
        v.extend_from_slice(&self.t_cost.to_le_bytes());
        v.extend_from_slice(&self.p_cost.to_le_bytes());
        v.extend_from_slice(&self.vault_id);
        v
    }

    pub fn password_slot_aad(&self) -> Vec<u8> {
        let mut v = self.aad_base();
        v.extend_from_slice(&self.password_salt);
        v.extend_from_slice(&self.password_nonce);
        v
    }

    pub fn recovery_slot_aad(&self) -> Vec<u8> {
        let mut v = self.aad_base();
        v.extend_from_slice(&self.recovery_salt);
        v.extend_from_slice(&self.recovery_nonce);
        v
    }

    pub fn payload_aad(&self) -> Vec<u8> {
        let mut v = self.aad_base();
        v.extend_from_slice(&self.payload_nonce);
        v
    }

    pub fn to_prefix(&self) -> Vec<u8> {
        let mut v = self.aad_base();
        v.extend_from_slice(&self.key_generation.to_le_bytes());
        v.extend_from_slice(&self.password_salt);
        v.extend_from_slice(&self.password_nonce);
        v.extend_from_slice(&self.recovery_salt);
        v.extend_from_slice(&self.recovery_nonce);
        v.extend_from_slice(&self.payload_nonce);
        debug_assert_eq!(v.len(), PREFIX_LEN);
        v
    }

    pub fn from_prefix(buf: &[u8]) -> Result<Self, VaultError> {
        if buf.len() < PREFIX_LEN {
            return Err(VaultError::Format("文件已损坏或被截断。".into()));
        }
        if buf[0..4] != MAGIC {
            return Err(VaultError::Format("不是有效的密码本文件。".into()));
        }
        if buf[4] != FORMAT_VERSION {
            return Err(VaultError::Format(format!(
                "不支持的密码本格式版本：{}",
                buf[4]
            )));
        }

        let mut r = Reader::new(&buf[5..]);
        let header = Self {
            m_cost_kib: r.u32()?,
            t_cost: r.u32()?,
            p_cost: r.u32()?,
            vault_id: r.array()?,
            key_generation: r.u32()?,
            password_salt: r.array()?,
            password_nonce: r.array()?,
            recovery_salt: r.array()?,
            recovery_nonce: r.array()?,
            payload_nonce: r.array()?,
        };

        if header.m_cost_kib == 0 || header.t_cost == 0 || header.p_cost == 0 {
            return Err(VaultError::Format("密码本头部无效。".into()));
        }

        Ok(header)
    }
}

pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], VaultError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|e| *e <= self.buf.len())
            .ok_or_else(|| VaultError::Format("文件已损坏或被截断。".into()))?;
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    pub fn u32(&mut self) -> Result<u32, VaultError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn i32(&mut self) -> Result<i32, VaultError> {
        Ok(self.u32()? as i32)
    }

    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], VaultError> {
        let b = self.take(N)?;
        let mut out = [0u8; N];
        out.copy_from_slice(b);
        Ok(out)
    }

    pub fn bytes(&mut self, n: usize) -> Result<Vec<u8>, VaultError> {
        Ok(self.take(n)?.to_vec())
    }

    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_is_105_bytes_and_roundtrips() {
        let h = VaultHeader::generate().unwrap();
        let prefix = h.to_prefix();
        assert_eq!(prefix.len(), PREFIX_LEN);

        let parsed = VaultHeader::from_prefix(&prefix).unwrap();
        assert_eq!(parsed.vault_id, h.vault_id);
        assert_eq!(parsed.m_cost_kib, h.m_cost_kib);
        assert_eq!(parsed.password_salt, h.password_salt);
        assert_eq!(parsed.payload_nonce, h.payload_nonce);
    }

    #[test]
    fn aads_are_distinct_and_independent() {
        let h = VaultHeader::generate().unwrap();
        assert_eq!(h.password_slot_aad().len(), 33 + SALT_LEN + NONCE_LEN);
        assert_eq!(h.recovery_slot_aad().len(), 33 + SALT_LEN + NONCE_LEN);
        assert_eq!(h.payload_aad().len(), 33 + NONCE_LEN);
        assert_ne!(h.password_slot_aad(), h.recovery_slot_aad());
    }

    #[test]
    fn key_generation_is_not_authenticated() {
        // key_generation 只是「本机免密缓存是否失效」的提示,刻意不放进任何 AAD,
        // 这样改主密码时只需重包主密码槽,恢复码槽与载荷都不受影响。
        let mut h = VaultHeader::generate().unwrap();
        let password_before = h.password_slot_aad();
        let recovery_before = h.recovery_slot_aad();
        let payload_before = h.payload_aad();

        h.key_generation += 1;

        assert_eq!(password_before, h.password_slot_aad());
        assert_eq!(recovery_before, h.recovery_slot_aad());
        assert_eq!(payload_before, h.payload_aad());
    }

    #[test]
    fn bad_magic_is_rejected() {
        let mut prefix = VaultHeader::generate().unwrap().to_prefix();
        prefix[0] = b'X';
        assert!(VaultHeader::from_prefix(&prefix).is_err());
    }
}
