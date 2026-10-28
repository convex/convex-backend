use std::{
    ops::Deref,
    str::FromStr,
};

use crate::{
    identifier::MAX_IDENTIFIER_LEN,
    FunctionName,
};

#[derive(Debug, thiserror::Error)]
pub enum InvalidPathComponentError {
    #[error("Path component is too long ({length} > maximum {MAX_IDENTIFIER_LEN}): {prefix}...")]
    TooLong { length: usize, prefix: String },
    #[error(
        "Path component {component} can only contain alphanumeric characters, underscores, or \
         periods."
    )]
    InvalidCharacter { component: String },
    #[error("Path component {component} must have at least one alphanumeric character.")]
    MissingAlphanumeric { component: String },
}

pub fn check_valid_path_component(s: &str) -> Result<(), InvalidPathComponentError> {
    if s.len() > MAX_IDENTIFIER_LEN {
        return Err(InvalidPathComponentError::TooLong {
            length: s.len(),
            prefix: s
                .chars()
                .scan(0, |bytes, c| {
                    *bytes += c.len_utf8();
                    (*bytes <= MAX_IDENTIFIER_LEN).then_some(c)
                })
                .collect(),
        });
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    {
        return Err(InvalidPathComponentError::InvalidCharacter {
            component: s.to_owned(),
        });
    }
    if !s.chars().any(|c| c.is_ascii_alphanumeric()) {
        return Err(InvalidPathComponentError::MissingAlphanumeric {
            component: s.to_owned(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct PathComponent(String);

impl FromStr for PathComponent {
    type Err = InvalidPathComponentError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        check_valid_path_component(s)?;
        Ok(Self(s.to_owned()))
    }
}

impl Deref for PathComponent {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<PathComponent> for String {
    fn from(p: PathComponent) -> Self {
        p.0
    }
}

impl From<FunctionName> for PathComponent {
    fn from(function_name: FunctionName) -> Self {
        function_name
            .parse()
            .expect("FunctionName isn't a valid PathComponent")
    }
}
