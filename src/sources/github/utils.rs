//! GitHub-specific helpers: signature verification and branch extraction.

/// Verifies the `X-Hub-Signature-256` header.
///
/// GitHub sends the signature as `sha256=<hex>`. This strips the prefix
/// and delegates to the shared HMAC verifier.
pub fn verify_github_signature(secret: &str, body: &[u8], header_value: &str) -> bool {
    let Some(hex_sig) = header_value.strip_prefix("sha256=") else {
        return false;
    };
    crate::utils::verify_hmac_sha256(secret, body, hex_sig)
}

/// Extracts the short branch name from a full git ref
/// (e.g. `"refs/heads/main"` → `"main"`).
pub fn branch_from_ref(git_ref: &str) -> &str {
    git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref)
}
