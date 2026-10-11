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
    ///
    /// 写端**不做**读端那套范围校验:传入界外参数会造出「自己都读不回」的
    /// 库(读端的 `from_prefix` 会按损坏拒绝)。正式创建走 [`Self::generate`]
    /// 的默认值。
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

        // KDF 参数在密文校验**之前**就会被拿去算密钥:上界必须在这里卡住。
        // 坏头部若允许 1 GiB / 64 轮 / 64 路,解锁会在 UI 线程上按它把内存
        // 与 CPU 跑满(表现为程序假死,低内存机器直接 OOM —— release 构建是
        // panic = "abort",没有提示)。上界单维度不超过默认值(64 MiB / 3 /
        // 4)的 4 倍、p 为 2 倍,组合最坏约 16 倍默认派生耗时;此处同时要求
        // m ≥ 8p(Argon2 自身的约束),组合非法也按头部无效拒绝,不让它拖到
        // 派生阶段才报错。正常写入方(含集成测试的 8 KiB / 1 / 1)都远小于上界。
        if !(8..=262_144).contains(&header.m_cost_kib)
            || !(1..=12).contains(&header.t_cost)
            || !(1..=8).contains(&header.p_cost)
            || header.m_cost_kib < header.p_cost * 8
        {
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

    #[test]
    fn absurd_kdf_params_are_rejected() {
        // 密文校验之前就会按这些参数派生密钥:坏头部不能让解锁先跑满 CPU。
        // 头部布局:magic(4) + version(1) + m(4) + t(4) + p(4) ……。
        let mut prefix = VaultHeader::generate().unwrap().to_prefix();
        prefix[5..9].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(VaultHeader::from_prefix(&prefix).is_err());

        let mut prefix = VaultHeader::generate().unwrap().to_prefix();
        prefix[9..13].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(VaultHeader::from_prefix(&prefix).is_err());

        let mut prefix = VaultHeader::generate().unwrap().to_prefix();
        prefix[13..17].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(VaultHeader::from_prefix(&prefix).is_err());
    }

    /// 单维度上界不超过默认值的 4 倍(p 为 2 倍),下界保住集成测试的廉价
    /// 参数:每个维度「上界内通过、越界 1 拒绝」,下界外的值同样拒绝。
    #[test]
    fn kdf_bounds_cover_normal_params_and_reject_excess() {
        let defaults = VaultHeader::generate().unwrap().to_prefix();
        assert!(VaultHeader::from_prefix(&defaults).is_ok());

        let cheap = VaultHeader::generate_with(8, 1, 1).unwrap().to_prefix();
        assert!(VaultHeader::from_prefix(&cheap).is_ok());

        let patch = |offset: usize, value: u32| {
            let mut prefix = VaultHeader::generate().unwrap().to_prefix();
            prefix[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            VaultHeader::from_prefix(&prefix)
        };

        assert!(patch(5, 262_144).is_ok(), "m 在上界内应通过");
        assert!(patch(5, 262_145).is_err(), "m 超过 256 MiB 应拒绝");
        assert!(patch(5, 7).is_err(), "m 低于 8 KiB 应拒绝");

        assert!(patch(9, 12).is_ok(), "t 在上界内应通过");
        assert!(patch(9, 13).is_err(), "t 超过 12 轮应拒绝");
        assert!(patch(9, 0).is_err(), "t = 0 应拒绝");

        assert!(patch(13, 8).is_ok(), "p 在上界内应通过");
        assert!(patch(13, 9).is_err(), "p 超过 8 路应拒绝");
        assert!(patch(13, 0).is_err(), "p = 0 应拒绝");

        // Argon2 要求 m ≥ 8p:两个维度各自都在界内、组合非法同样要拒绝。
        let combo = VaultHeader::generate_with(8, 1, 8).unwrap().to_prefix();
        assert!(VaultHeader::from_prefix(&combo).is_err(), "m=8 / p=8 组合非法");
    }
}
