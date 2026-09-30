//! 网址规范化:决定「条目里存的网址能不能交给系统浏览器打开」。
//!
//! 为什么放在可移植核心:`ShellExecuteW` 会把字符串交给 Windows 的协议处理器,
//! 能打开什么完全由这张允许名单说了算 —— 这是一处安全判定,必须能在 Linux 上测。

/// 把条目里存的网址整理成可以直接交给浏览器的 `http(s)://` 链接。
///
/// 返回 `None` 表示**不该**由我们打开:
/// - 空串(或只有空白);
/// - 含控制字符(换行、制表符等);
/// - 带别的协议。`file:` 会让系统直接执行本地文件,`javascript:` `mailto:`
///   `ftp:` 之类也各有各的处理器 —— 一律不放行。
///
/// 没写协议的按 `https://` 补全(`github.com` → `https://github.com`),但
/// 「主机:端口」不会被误判成协议(`192.168.1.1:8080` → `https://192.168.1.1:8080`)。
pub fn normalize_http_url(raw: &str) -> Option<String> {
    let text = raw.trim();
    if text.is_empty() || text.chars().any(char::is_control) {
        return None;
    }
    // 以路径分隔符开头的一律不是网址(本地路径 / UNC 路径),不替用户猜。
    if text.starts_with(['/', '\\']) {
        return None;
    }

    let lower = text.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        // 大小写照原样保留 —— 打不开也不是我们该改的。
        return Some(text.to_string());
    }

    if scheme_of(text).is_some() {
        // 明明写了协议,但不是 http(s):不碰。
        return None;
    }

    Some(format!("https://{text}"))
}

/// 取出开头的协议名(小写)。
///
/// 按 URL 语法:字母开头,后跟字母/数字/`+`/`-`/`.` 再以 `:` 结束。
/// 例外:`example.com:8443` 这类「主机:端口」不算协议 —— 判断办法是冒号后面
/// 到下一个 `/` `?` `#` 之间全是数字;`192.168.1.1:8080` 则因为协议名不能以
/// 数字开头而直接落到「不是协议」。
fn scheme_of(text: &str) -> Option<String> {
    let colon = text.find(':')?;
    let (head, rest) = text.split_at(colon);

    let starts_ok = head.starts_with(|c: char| c.is_ascii_alphabetic());
    let chars_ok = head
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !starts_ok || !chars_ok {
        return None;
    }

    let after = &rest[1..];
    let port_like = after.split(['/', '?', '#']).next().unwrap_or("");
    if !port_like.is_empty() && port_like.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    Some(head.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_host_gets_https() {
        assert_eq!(
            normalize_http_url("github.com").as_deref(),
            Some("https://github.com")
        );
        assert_eq!(
            normalize_http_url("  example.com/login  ").as_deref(),
            Some("https://example.com/login")
        );
    }

    #[test]
    fn explicit_http_and_https_are_kept_verbatim() {
        assert_eq!(
            normalize_http_url("http://example.com").as_deref(),
            Some("http://example.com")
        );
        assert_eq!(
            normalize_http_url("HTTPS://Example.COM/x?y=1").as_deref(),
            Some("HTTPS://Example.COM/x?y=1")
        );
    }

    #[test]
    fn host_with_port_is_not_mistaken_for_a_scheme() {
        assert_eq!(
            normalize_http_url("192.168.1.1:8080/admin").as_deref(),
            Some("https://192.168.1.1:8080/admin")
        );
        assert_eq!(
            normalize_http_url("example.com:8443").as_deref(),
            Some("https://example.com:8443")
        );
    }

    #[test]
    fn other_schemes_are_refused() {
        for raw in [
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "mailto:someone@example.com",
            "ftp://example.com",
            "web+demo://example.com",
        ] {
            assert_eq!(normalize_http_url(raw), None, "{raw} 不该被放行");
        }
    }

    #[test]
    fn local_paths_are_refused() {
        for raw in [
            r"C:\Windows\System32\calc.exe",
            r"\\server\share\evil.exe",
            "/usr/bin/sh",
        ] {
            assert_eq!(normalize_http_url(raw), None, "{raw} 不该被放行");
        }
    }

    #[test]
    fn empty_and_control_characters_are_refused() {
        assert_eq!(normalize_http_url(""), None);
        assert_eq!(normalize_http_url("   "), None);
        assert_eq!(normalize_http_url("https://example.com\nrm -rf /"), None);
        assert_eq!(normalize_http_url("https://example.com\t/x"), None);
        assert_eq!(normalize_http_url("https://example.com/\0"), None);
    }

    #[test]
    fn spaces_inside_a_url_are_left_alone() {
        // 浏览器自己会把空格编码成 %20,不必替它决定,也不该因此拒绝打开。
        assert_eq!(
            normalize_http_url("https://example.com/a b").as_deref(),
            Some("https://example.com/a b")
        );
    }
}
