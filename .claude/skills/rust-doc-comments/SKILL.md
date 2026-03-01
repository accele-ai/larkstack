---
name: rust-doc-comments
description: Audit and fix Rust doc comments to follow rustdoc conventions, ensuring docs.rs compatibility. Use when writing new Rust code, reviewing existing comments, or after refactoring.
allowed-tools: Read, Grep, Glob, Edit, Bash
argument-hint: "[file or directory path]"
---

# Rust Doc Comment Conventions

Audit the target path (default: `src/`) and rewrite all comments to follow rustdoc / docs.rs conventions.

Target: `$ARGUMENTS` (fallback to `src/` if empty)

## What to fix

### 1. Delete style/separator lines

These are purely decorative and generate no documentation:

```rust
// BAD
// ---------------------------------------------------------------------------
// Section title
// ---------------------------------------------------------------------------

// GOOD — just delete them, use doc comments on the item below instead
```

### 2. Module-level docs use `//!`

Every module file (`mod.rs`, `lib.rs`, named modules) should open with `//!`:

```rust
//! Brief one-line summary of this module's responsibility.
//!
//! Optional longer description with [`links`](crate::path) to related items.
```

Keep it concise — 1–3 lines for leaf modules, up to a short paragraph for important ones.

### 3. Public items use `///`

All `pub` types, functions, methods, enum variants, and significant fields get `///`:

```rust
/// Truncates `s` to at most `max_chars` characters, appending `"…"` on overflow.
pub fn truncate(s: &str, max_chars: usize) -> String { ... }
```

Rules:
- First line is a **single sentence** summary (shows in module index on docs.rs).
- Use `` `backticks` `` for parameter names, types, and code fragments.
- Use [`intra-doc links`] for cross-references: `[`Event`]`, `[`Event::IssueCreated`]`, `[`crate::event`]`.
- Document panics under `# Panics`, errors under `# Errors`, only when non-obvious.
- Don't doc trivial getters, simple struct fields, or items whose name is already self-explanatory.

### 4. Internal comments stay as `//`

Implementation details, inline clarifications, and TODOs remain plain `//`:

```rust
// Linear sometimes sends state as a flat string
.or_else(|| old_state.as_str())
```

Don't convert these to `///` — they describe *how*, not *what*.

### 5. No redundant / echo comments

```rust
// BAD — just repeats the code
/// Creates a new Foo.
pub fn new() -> Foo { ... }

// GOOD — adds value
/// Initializes with sensible defaults; use [`with_config`](Self::with_config)
/// for custom settings.
pub fn new() -> Foo { ... }

// ALSO GOOD — obvious constructor, just skip the doc
pub fn new() -> Foo { ... }
```

## Execution steps

1. `Grep` for `// ---` separator lines across the target — delete all of them.
2. Check each `.rs` file for `//!` module docs — add where missing.
3. `Grep` for `pub fn`, `pub struct`, `pub enum`, `pub trait` without a preceding `///` — add doc comments.
4. Verify no `///` on private internals unless genuinely helpful.
5. Run `cargo clippy` — zero warnings.
6. Run `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` — zero warnings.
