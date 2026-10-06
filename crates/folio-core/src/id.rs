//! Short ids for pages, blocks, slides, shapes, media and comments.
//!
//! Eight lowercase letters and digits: short enough for an agent to copy, random enough never to
//! collide inside one file.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct Id(pub String);

impl Id {
    pub fn new() -> Self {
        let u = uuid::Uuid::new_v4().as_u128();
        const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
        let s: String = (0..8).map(|i| ALPHABET[((u >> (i * 5)) & 31) as usize] as char).collect();
        Id(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Id {
    fn from(s: &str) -> Self {
        Id(s.to_string())
    }
}

impl From<String> for Id {
    fn from(s: String) -> Self {
        Id(s)
    }
}

impl PartialEq<str> for Id {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Id {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}
