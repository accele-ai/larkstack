use hmac::{Hmac, Mac};
use sha2::Sha256;

/// Computes HMAC-SHA256 over `body` using `secret` and compares the
/// hex-encoded result against `expected_hex`.
pub fn verify_hmac_sha256(secret: &str, body: &[u8], expected_hex: &str) -> bool {
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    let Ok(expected_bytes) = hex::decode(expected_hex) else {
        return false;
    };
    mac.verify_slice(&expected_bytes).is_ok()
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
