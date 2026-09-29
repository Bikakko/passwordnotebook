//! 恢复码:20 字节随机数据的 Base32 表示(32 个字符,分组显示)。
//!
//! 忘记主密码时,可用恢复码解开数据密钥并重设主密码。

use crate::crypto;

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const BYTE_COUNT: usize = 20;
const CHAR_COUNT: usize = 32;

pub fn generate() -> Result<String, crate::error::VaultError> {
    let bytes = crypto::random(BYTE_COUNT)?;
    Ok(format_grouped(&encode(&bytes)))
}

/// 把 32 个字符按每 4 个一组加上连字符。
pub fn format_grouped(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + raw.len() / 4);
    for (i, ch) in raw.chars().enumerate() {
        if i > 0 && i % 4 == 0 {
            out.push('-');
        }
        out.push(ch);
    }
    out
}

/// 规范化用户输入:去掉分隔符、转大写、修正常见误输入。成功返回 32 个字符的规范形式。
pub fn normalize(input: &str) -> Option<String> {
    let mut out = String::with_capacity(CHAR_COUNT);
    for ch in input.chars() {
        if ch == '-' || ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r' {
            continue;
        }
        let upper = ch.to_ascii_uppercase();
        out.push(match upper {
            '0' => 'O',
            '1' => 'I',
            '8' => 'B',
            '9' => 'G',
            c => c,
        });
    }

    if out.len() != CHAR_COUNT || !out.bytes().all(|b| ALPHABET.contains(&b)) {
        return None;
    }
    Some(out)
}

/// 判断是否是格式合法的恢复码。
pub fn is_valid(input: &str) -> bool {
    normalize(input).is_some()
}

fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() * 8 + 4) / 5);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;

    for b in data {
        buffer = (buffer << 8) | *b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 0x1F) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 0x1F) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_code_has_32_chars_in_8_groups() {
        let code = generate().unwrap();
        assert_eq!(code.len(), 32 + 7);
        assert_eq!(code.matches('-').count(), 7);
        assert!(code.split('-').all(|g| g.len() == 4));
    }

    #[test]
    fn normalize_accepts_grouped_lowercase_and_typos() {
        let raw = "abcd-efgh-ijkl-mnop-qrst-uvwx-yz23-4567";
        let n = normalize(raw).unwrap();
        assert_eq!(n.len(), 32);
        assert_eq!(normalize(&n.to_lowercase()).unwrap(), n);
        assert_eq!(normalize(&raw.replace('-', " ")).unwrap(), n);
    }

    #[test]
    fn normalize_maps_confusable_digits() {
        // 0→O, 1→I, 8→B, 9→G
        assert_eq!(normalize("0000-0000-0000-0000-0000-0000-0000-0000").unwrap(), "O".repeat(32));
        assert_eq!(normalize("1111-1111-1111-1111-1111-1111-1111-1111").unwrap(), "I".repeat(32));
    }

    #[test]
    fn normalize_rejects_wrong_length_or_chars() {
        assert!(normalize("ABC").is_none());
        assert!(normalize("").is_none());
        // 32 个字符但含非法字母(字母表里没有 '1' 之外的数字映射后仍非法)
        assert!(normalize(&"!".repeat(32)).is_none());
    }

    #[test]
    fn two_generated_codes_differ() {
        assert_ne!(generate().unwrap(), generate().unwrap());
    }
}
