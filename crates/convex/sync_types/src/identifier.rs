use std::{
    fmt::Display,
    ops::Deref,
    str::FromStr,
};

pub const MAX_IDENTIFIER_LEN: usize = 64;

pub const IDENTIFIER_REQUIREMENTS: &str =
    "Identifiers must start with a letter and can only contain letters, digits, and underscores.";

/// Check that a string can be used for a table name or field name in a
/// document.
///
/// We use a simplified ASCII version of Rust's syntax[^1] which also overlaps
/// with JavaScript's syntax[^2].
///
/// ```text
/// ident: start continue*
/// start: a-zA-z_
/// continue: a-zA-Z0-9_
/// ```
/// [^3] [^4]
/// To be conservative, let's also ban identifiers of entirely `_` too.
///
/// [^1]: <https://doc.rust-lang.org/reference/identifiers.html>
/// [^2]: <https://developer.mozilla.org/en-US/docs/Glossary/Identifier>
/// [^3]: <https://util.unicode.org/UnicodeJsps/list-unicodeset.jsp?a=%5B%3AXID_START%3A%5D&g=&i=>
/// [^4]: <https://util.unicode.org/UnicodeJsps/list-unicodeset.jsp?a=%5B%3AXID_CONTINUE%3A%5D&g=&i=>
pub fn check_valid_identifier(s: &str) -> anyhow::Result<()> {
    if is_valid_identifier(s) {
        Ok(())
    } else {
        check_valid_identifier_slow(s)
    }
}

pub const fn is_valid_identifier(s: &str) -> bool {
    if s.len() > MAX_IDENTIFIER_LEN {
        return false;
    }
    let bytes = s.as_bytes();
    match bytes.first() {
        Some(c) if c.is_ascii_alphabetic() => (),
        Some(b'_') => (),
        _ => return false,
    }
    let mut has_non_underscore = false;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphanumeric() {
            has_non_underscore = true;
        } else if bytes[i] != b'_' {
            return false;
        }
        i += 1;
    }
    if !has_non_underscore {
        return false;
    }
    true
}

#[derive(Clone, Debug, PartialEq, PartialOrd, Ord, Eq, Hash)]
pub struct Identifier(String);

impl Identifier {
    pub fn min() -> Self {
        Identifier(MIN_IDENTIFIER.to_string())
    }

    pub fn capacity(&self) -> usize {
        self.0.capacity()
    }
}

impl FromStr for Identifier {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        check_valid_identifier(s)?;
        Ok(Identifier(s.to_string()))
    }
}

impl From<Identifier> for String {
    fn from(id: Identifier) -> String {
        id.0
    }
}

impl Display for Identifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Deref for Identifier {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

#[cold]
fn check_valid_identifier_slow(s: &str) -> anyhow::Result<()> {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => (),
        Some('_') => (),
        Some(c) => {
            anyhow::bail!(
                "Invalid first character {c:?} in {s}: Identifiers must start with an alphabetic \
                 character or underscore"
            )
        },
        None => anyhow::bail!("Identifier cannot be empty"),
    };
    for c in chars {
        if !c.is_ascii_alphanumeric() && c != '_' {
            anyhow::bail!(
                "Identifier {s} has invalid character {c:?}: Identifiers can only contain \
                 alphanumeric characters or underscores"
            );
        }
    }
    if s.len() > MAX_IDENTIFIER_LEN {
        anyhow::bail!(
            "Identifier is too long ({} > maximum {})",
            s.len(),
            MAX_IDENTIFIER_LEN
        );
    }
    if s.chars().all(|c| c == '_') {
        anyhow::bail!("Identifier {s} cannot have exclusively underscores");
    }
    Ok(())
}

pub const MIN_IDENTIFIER: &str = "A";

pub const MAX_FIELD_NAME_LENGTH: usize = 1024;

/// Check that a string can be used as field in a Convex object.
///
/// Field names cannot start with '$', must contain only non-control ASCII
/// characters, and must be at most 1024 characters long.
pub fn check_valid_field_name(s: &str) -> anyhow::Result<()> {
    if is_valid_field_name(s) {
        return Ok(());
    }
    check_valid_field_name_slow(s)
}

pub const fn is_valid_field_name(s: &str) -> bool {
    if let [b'$', ..] = s.as_bytes() {
        return false;
    }
    if s.len() > MAX_FIELD_NAME_LENGTH {
        return false;
    }
    let mut bytes = s.as_bytes();
    // LLVM is able to vectorize this
    while let Some((chunk, rest)) = bytes.split_first_chunk::<16>() {
        bytes = rest;
        let mut j = 0;
        let mut bad = false;
        while j < 16 {
            let c = chunk[j];
            bad |= !c.is_ascii() || c.is_ascii_control();
            j += 1;
        }
        if bad {
            return false;
        }
    }
    while let Some((&c, rest)) = bytes.split_first() {
        bytes = rest;
        // `|` generates better code than `||` when used directly in an `if` condition
        if !c.is_ascii() | c.is_ascii_control() {
            return false;
        }
    }
    true
}

#[cold]
fn check_valid_field_name_slow(s: &str) -> anyhow::Result<()> {
    if s.starts_with('$') {
        anyhow::bail!("Field name {s} starts with '$', which is reserved.");
    }
    for c in s.chars() {
        if !c.is_ascii() || c.is_ascii_control() {
            anyhow::bail!(
                "Field name {s} has invalid character {c:?}: Field names can only contain \
                 non-control ASCII characters"
            );
        }
    }
    if s.len() > MAX_FIELD_NAME_LENGTH {
        anyhow::bail!(
            "Field name is too long ({} > maximum {})",
            s.len(),
            MAX_FIELD_NAME_LENGTH
        );
    }
    Ok(())
}
