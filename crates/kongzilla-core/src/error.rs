//! Error types shared across the crate.

use core::fmt;

/// Anything that can go wrong while reading user-supplied text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// A card could not be read (bad rank or suit character, wrong length).
    BadCard(String),
    /// A hand class such as `AKs` could not be read.
    BadHandClass(String),
    /// A range token such as `77-99` could not be read.
    BadRangeToken(String),
    /// A weight suffix such as `:0.5` could not be read.
    BadWeight(String),
    /// The same card appeared twice where it may not.
    DuplicateCard(String),
    /// A board was given more cards than a board can hold.
    BoardTooLong(usize),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadCard(s) => write!(f, "not a card: {s:?}"),
            Self::BadHandClass(s) => write!(f, "not a hand class: {s:?}"),
            Self::BadRangeToken(s) => write!(f, "not a range token: {s:?}"),
            Self::BadWeight(s) => write!(f, "not a weight: {s:?}"),
            Self::DuplicateCard(s) => write!(f, "duplicate card: {s:?}"),
            Self::BoardTooLong(n) => write!(f, "a board holds at most 5 cards, got {n}"),
        }
    }
}

impl std::error::Error for ParseError {}
