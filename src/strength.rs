//! 登录密码强度评估（粗略估计，用于界面提示）。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strength {
    pub score: u8,
    pub label: &'static str,
    pub hint: &'static str,
}

pub fn evaluate(password: &str) -> Strength {
    if password.is_empty() {
        return Strength {
            score: 0,
            label: "未输入",
            hint: "建议至少 12 位，混合大小写字母、数字与符号。",
        };
    }

    let mut classes = 0;
    if password.chars().any(|c| c.is_ascii_lowercase()) {
        classes += 1;
    }
    if password.chars().any(|c| c.is_ascii_uppercase()) {
        classes += 1;
    }
    if password.chars().any(|c| c.is_ascii_digit()) {
        classes += 1;
    }
    if password.chars().any(|c| !c.is_alphanumeric()) {
        classes += 1;
    }

    let len = password.chars().count();
    let mut score: i32 = 0;
    if len >= 8 {
        score += 1;
    }
    if len >= 12 {
        score += 1;
    }
    if len >= 16 {
        score += 1;
    }
    if classes >= 3 {
        score += 1;
    }
    let score = score.clamp(0, 4) as u8;

    let label = match score {
        0 | 1 => "弱",
        2 => "中等",
        3 => "较强",
        _ => "很强",
    };
    let hint = if score <= 1 {
        "建议至少 12 位，混合大小写字母、数字与符号。"
    } else {
        "忘记登录密码时，用恢复码找回。"
    };

    Strength { score, label, hint }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_is_weak() {
        assert!(evaluate("abc").score <= 1);
    }

    #[test]
    fn long_mixed_is_strong() {
        assert!(evaluate("Tr0ub4dor&3xYz!").score >= 3);
    }

    #[test]
    fn empty_is_zero() {
        assert_eq!(evaluate("").score, 0);
    }
}
