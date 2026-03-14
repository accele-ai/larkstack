use hmac::{Hmac, Mac};
use sha2::Sha256;

/// Computes HMAC-SHA256 over `body` using `secret` and compares the
/// hex-encoded result against `expected_hex`.
pub fn verify_hmac_sha256(secret: &str, body: &[u8], expected_hex: &str) -> bool {
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    let computed = hex::encode(mac.finalize().into_bytes());
    computed == expected_hex
}

/// Truncates `s` to at most `max_chars` characters, appending `"…"` when
/// truncation occurs.
pub fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- HMAC-SHA256 ----------------------------------------------------------

    #[test]
    fn hmac_valid_signature() {
        let secret = "test-secret";
        let body = b"hello world";
        // Pre-computed: HMAC-SHA256("test-secret", "hello world")
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let expected = hex::encode(mac.finalize().into_bytes());

        assert!(verify_hmac_sha256(secret, body, &expected));
    }

    #[test]
    fn hmac_wrong_signature() {
        assert!(!verify_hmac_sha256("secret", b"body", "0000deadbeef"));
    }

    #[test]
    fn hmac_empty_body() {
        let secret = "key";
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(b"");
        let expected = hex::encode(mac.finalize().into_bytes());

        assert!(verify_hmac_sha256(secret, b"", &expected));
    }

    // -- truncate -------------------------------------------------------------

    #[test]
    fn truncate_within_limit() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_exact_limit() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_over_limit() {
        assert_eq!(truncate("hello world", 5), "hello…");
    }

    #[test]
    fn truncate_empty_string() {
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn truncate_multibyte_chars() {
        // CJK characters are single chars but multi-byte in UTF-8
        assert_eq!(truncate("你好世界测试", 4), "你好世界…");
    }
}
