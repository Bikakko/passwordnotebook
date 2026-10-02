//! 密码生成器。

use crate::crypto;
use crate::error::{Result, VaultError};

const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
const DIGITS: &str = "0123456789";
const SYMBOLS: &str = "!@#$%^&*()-_=+[]{};:,.?/";
const AMBIGUOUS: &str = "Il1O0o|`'\"";

#[derive(Clone, Debug)]
pub struct Options {
    pub length: usize,
    pub upper: bool,
    pub lower: bool,
    pub digits: bool,
    pub symbols: bool,
    pub exclude_ambiguous: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            length: 20,
            upper: true,
            lower: true,
            digits: true,
            symbols: true,
            exclude_ambiguous: true,
        }
    }
}

pub fn generate(options: &Options) -> Result<String> {
    let mut pools: Vec<String> = Vec::new();
    if options.upper {
        pools.push(UPPER.to_string());
    }
    if options.lower {
        pools.push(LOWER.to_string());
    }
    if options.digits {
        pools.push(DIGITS.to_string());
    }
    if options.symbols {
        pools.push(SYMBOLS.to_string());
    }
    if pools.is_empty() {
        pools.push(LOWER.to_string());
    }

    if options.exclude_ambiguous {
        pools = pools
            .iter()
            .map(|p| p.chars().filter(|c| !AMBIGUOUS.contains(*c)).collect::<String>())
            .filter(|p| !p.is_empty())
            .collect();
        if pools.is_empty() {
            return Err(VaultError::Invalid("没有可用的字符集。".into()));
        }
    }

    let length = options.length.clamp(4, 256);
    let all: String = pools.concat();

    let mut chars: Vec<char> = Vec::with_capacity(length);

    // 先保证每个选中的字符集至少出现一次。
    for pool in &pools {
        if chars.len() >= length {
            break;
        }
        chars.push(pick(pool)?);
    }
    while chars.len() < length {
        chars.push(pick(&all)?);
    }

    shuffle(&mut chars)?;
    Ok(chars.into_iter().collect())
}

fn pick(pool: &str) -> Result<char> {
    let bytes = pool.as_bytes();
    let idx = uniform_index(bytes.len())?;
    Ok(bytes[idx] as char)
}

/// 无模偏差地取 [0, n) 内的随机下标。
fn uniform_index(n: usize) -> Result<usize> {
    debug_assert!(n > 0);
    if n == 1 {
        return Ok(0);
    }
    let n = n as u64;
    let limit = u64::MAX - (u64::MAX % n);
    loop {
        let bytes = crypto::random_array::<8>()?;
        let value = u64::from_le_bytes(bytes);
        if value < limit {
            return Ok((value % n) as usize);
        }
    }
}

fn shuffle(chars: &mut [char]) -> Result<()> {
    for i in (1..chars.len()).rev() {
        let j = uniform_index(i + 1)?;
        chars.swap(i, j);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respects_length_and_character_sets() {
        for _ in 0..200 {
            let pwd = generate(&Options::default()).unwrap();
            assert_eq!(pwd.chars().count(), 20);
            assert!(pwd.chars().any(|c| c.is_ascii_uppercase()));
            assert!(pwd.chars().any(|c| c.is_ascii_lowercase()));
            assert!(pwd.chars().any(|c| c.is_ascii_digit()));
            assert!(pwd.chars().any(|c| !c.is_alphanumeric()));
            assert!(!pwd.chars().any(|c| AMBIGUOUS.contains(c)));
        }
    }

    #[test]
    fn single_pool_works() {
        let opts = Options {
            length: 12,
            upper: false,
            lower: false,
            digits: true,
            symbols: false,
            exclude_ambiguous: false,
        };
        let pwd = generate(&opts).unwrap();
        assert_eq!(pwd.len(), 12);
        assert!(pwd.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn length_is_clamped() {
        let opts = Options {
            length: 1,
            ..Options::default()
        };
        assert_eq!(generate(&opts).unwrap().chars().count(), 4);
    }

    #[test]
    fn passwords_differ_between_calls() {
        assert_ne!(
            generate(&Options::default()).unwrap(),
            generate(&Options::default()).unwrap()
        );
    }
}
