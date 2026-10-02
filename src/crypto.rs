//! AES-256-GCM 与 Argon2id 的薄封装。
//!
//! 不做任何密钥管理策略,只负责「给定密钥与随机数,密封/打开一段数据」。

use aes_gcm::aead::{Aead, KeyInit, Nonce, Payload};
use aes_gcm::{Aes256Gcm, Key};
use argon2::{Algorithm, Argon2, Params, Version};

use crate::error::VaultError;

/// GCM 随机数长度。
pub const NONCE_LEN: usize = 12;
/// GCM 认证标签长度。
pub const TAG_LEN: usize = 16;
/// 数据密钥长度。
pub const KEY_LEN: usize = 32;
/// 被包裹后的密钥长度(密文 + 标签)。
pub const WRAPPED_KEY_LEN: usize = KEY_LEN + TAG_LEN;
/// Argon2 盐长度。
pub const SALT_LEN: usize = 16;

/// Argon2id 默认参数:64 MiB 内存、3 次迭代、4 路并行。
pub const DEFAULT_M_COST_KIB: u32 = 65_536;
pub const DEFAULT_T_COST: u32 = 3;
pub const DEFAULT_P_COST: u32 = 4;

/// 生成 `len` 字节密码学安全随机数。
pub fn random(len: usize) -> Result<Vec<u8>, VaultError> {
    let mut buf = vec![0u8; len];
    getrandom::fill(&mut buf).map_err(|e| VaultError::Crypto(format!("随机数生成失败（{e}）")))?;
    Ok(buf)
}

/// 生成固定长度随机数组。
pub fn random_array<const N: usize>() -> Result<[u8; N], VaultError> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|e| VaultError::Crypto(format!("随机数生成失败（{e}）")))?;
    Ok(buf)
}

/// 用 Argon2id 从口令派生 32 字节密钥。
pub fn derive_key(
    secret: &[u8],
    salt: &[u8],
    m_cost_kib: u32,
    t_cost: u32,
    p_cost: u32,
) -> Result<[u8; KEY_LEN], VaultError> {
    let params = Params::new(m_cost_kib, t_cost, p_cost, Some(KEY_LEN))
        .map_err(|e| VaultError::Crypto(format!("Argon2 参数无效（{e}）")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut out = [0u8; KEY_LEN];
    argon
        .hash_password_into(secret, salt, &mut out)
        .map_err(|e| VaultError::Crypto(format!("密钥派生失败（{e}）")))?;
    Ok(out)
}

fn cipher(key: &[u8]) -> Result<Aes256Gcm, VaultError> {
    let key = Key::<Aes256Gcm>::try_from(key)
        .map_err(|_| VaultError::Crypto("密钥长度不是 32 字节。".into()))?;
    Ok(Aes256Gcm::new(&key))
}

fn nonce(nonce: &[u8]) -> Result<Nonce<Aes256Gcm>, VaultError> {
    Nonce::<Aes256Gcm>::try_from(nonce)
        .map_err(|_| VaultError::Crypto("随机数长度不是 12 字节。".into()))
}

/// 加密。返回密文 + 16 字节认证标签(标签追加在末尾)。
pub fn seal(key: &[u8], nonce_bytes: &[u8], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, VaultError> {
    cipher(key)?
        .encrypt(&nonce(nonce_bytes)?, Payload { msg: plaintext, aad })
        .map_err(|_| VaultError::Crypto("加密失败。".into()))
}

/// 解密。输入须为密文 + 标签;认证失败返回错误。
pub fn open(key: &[u8], nonce_bytes: &[u8], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>, VaultError> {
    cipher(key)?
        .decrypt(&nonce(nonce_bytes)?, Payload { msg: ciphertext, aad })
        .map_err(|_| VaultError::Crypto("认证失败：数据可能被篡改。".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_roundtrip() {
        let key = [7u8; KEY_LEN];
        let n = [1u8; NONCE_LEN];
        let ct = seal(&key, &n, b"hello world", b"aad").unwrap();
        assert_eq!(ct.len(), "hello world".len() + TAG_LEN);
        assert_eq!(open(&key, &n, &ct, b"aad").unwrap(), b"hello world");
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key = [7u8; KEY_LEN];
        let n = [1u8; NONCE_LEN];
        let mut ct = seal(&key, &n, b"secret", b"aad").unwrap();
        let last = ct.len() - 1;
        ct[last] ^= 0xFF;
        assert!(open(&key, &n, &ct, b"aad").is_err());
    }

    #[test]
    fn wrong_aad_fails() {
        let key = [7u8; KEY_LEN];
        let n = [1u8; NONCE_LEN];
        let ct = seal(&key, &n, b"secret", b"aad-1").unwrap();
        assert!(open(&key, &n, &ct, b"aad-2").is_err());
    }

    #[test]
    fn key_derivation_is_deterministic_and_salt_dependent() {
        let salt_a = [1u8; SALT_LEN];
        let salt_b = [2u8; SALT_LEN];
        let a1 = derive_key(b"pw", &salt_a, 8, 1, 1).unwrap();
        let a2 = derive_key(b"pw", &salt_a, 8, 1, 1).unwrap();
        let b = derive_key(b"pw", &salt_b, 8, 1, 1).unwrap();
        assert_eq!(a1, a2);
        assert_ne!(a1, b);
    }

    #[test]
    fn random_is_not_constant() {
        let a = random(32).unwrap();
        let b = random(32).unwrap();
        assert_ne!(a, b);
    }
}
