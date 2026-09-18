//! The community cards.

use crate::cards::{Card, CardSet, NUM_CARDS};
use crate::error::ParseError;
use core::fmt;

/// How many community cards are on the table.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Street {
    /// No cards yet.
    Preflop,
    /// Three cards.
    Flop,
    /// Four cards.
    Turn,
    /// Five cards.
    River,
}

impl Street {
    /// The street's name in lower case.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Preflop => "preflop",
            Self::Flop => "flop",
            Self::Turn => "turn",
            Self::River => "river",
        }
    }
}

/// Up to five distinct board ranks, highest first.
///
/// A fixed array rather than a `Vec`, because the classifier asks for this once
/// per hand and the preflop pass runs it tens of millions of times.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RankList {
    ranks: [u8; 5],
    len: u8,
}

impl core::ops::Deref for RankList {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.ranks[..self.len as usize]
    }
}

/// Zero to five community cards, in the order they were dealt.
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Board {
    cards: [u8; 5],
    len: u8,
}

impl Board {
    /// The maximum number of community cards.
    pub const MAX: usize = 5;

    /// An empty board.
    pub const fn empty() -> Self {
        Self {
            cards: [0; 5],
            len: 0,
        }
    }

    /// Builds a board from a slice, rejecting duplicates and over-long boards.
    pub fn from_cards(cards: &[Card]) -> Result<Self, ParseError> {
        if cards.len() > Self::MAX {
            return Err(ParseError::BoardTooLong(cards.len()));
        }
        let mut board = Self::empty();
        for card in cards {
            if board.mask().contains(*card) {
                return Err(ParseError::DuplicateCard(card.to_string()));
            }
            board.cards[board.len as usize] = card.index();
            board.len += 1;
        }
        Ok(board)
    }

    /// Reads a board such as `Kc Qh Jh`.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let mut cards = Vec::new();
        for token in text.split([' ', ',', '\t', '\n']).filter(|t| !t.is_empty()) {
            cards.push(Card::parse(token)?);
        }
        Self::from_cards(&cards)
    }

    /// The community cards, in dealt order.
    pub fn cards(&self) -> impl Iterator<Item = Card> + '_ {
        self.cards[..self.len as usize]
            .iter()
            .map(|&i| Card::from_index(i))
    }

    /// How many cards are on the board.
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// Whether the board is empty.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Which street the board is on.
    pub const fn street(&self) -> Street {
        match self.len {
            0 => Street::Preflop,
            3 => Street::Flop,
            4 => Street::Turn,
            _ => {
                if self.len >= 5 {
                    Street::River
                } else {
                    Street::Preflop
                }
            }
        }
    }

    /// Whether the board holds a legal number of cards for analysis.
    pub const fn is_dealt(&self) -> bool {
        self.len == 0 || self.len >= 3
    }

    /// The board as a card set.
    pub fn mask(&self) -> CardSet {
        self.cards()
            .map(Card::mask)
            .fold(CardSet::EMPTY, CardSet::union)
    }

    /// A 13-bit mask of the ranks present on the board.
    pub fn rank_mask(&self) -> u16 {
        self.cards().fold(0u16, |m, c| m | (1u16 << c.rank()))
    }

    /// The distinct board ranks, highest first.
    pub fn ranks_desc(&self) -> RankList {
        let mut mask = self.rank_mask();
        let mut list = RankList {
            ranks: [0; 5],
            len: 0,
        };
        while mask != 0 && (list.len as usize) < 5 {
            let rank = 15 - mask.leading_zeros() as u8;
            list.ranks[list.len as usize] = rank;
            list.len += 1;
            mask &= !(1u16 << rank);
        }
        list
    }

    /// How many times each rank appears on the board.
    pub fn rank_counts(&self) -> [u8; 13] {
        let mut counts = [0u8; 13];
        for card in self.cards() {
            counts[card.rank() as usize] += 1;
        }
        counts
    }

    /// How many cards of each suit are on the board.
    pub fn suit_counts(&self) -> [u8; 4] {
        let mut counts = [0u8; 4];
        for card in self.cards() {
            counts[card.suit() as usize] += 1;
        }
        counts
    }

    /// Returns a copy with `card` appended, or `None` if that is not possible.
    pub fn with_card(&self, card: Card) -> Option<Self> {
        if self.len as usize >= Self::MAX || self.mask().contains(card) {
            return None;
        }
        let mut next = *self;
        next.cards[next.len as usize] = card.index();
        next.len += 1;
        Some(next)
    }

    /// Returns a copy with the last card removed.
    pub fn without_last(&self) -> Self {
        self.truncated(self.len().saturating_sub(1))
    }

    /// Returns a copy truncated to `len` cards.
    ///
    /// Trimmed slots are zeroed so that two boards showing the same cards compare
    /// equal whatever they used to hold.
    pub fn truncated(&self, len: usize) -> Self {
        let mut next = *self;
        next.len = next.len.min(len as u8);
        for slot in next.cards.iter_mut().skip(next.len as usize) {
            *slot = 0;
        }
        next
    }

    /// Every three-card flop, in deck order. There are 22,100 of them.
    ///
    /// This is the enumeration the preflop statistics and the flop breakdown tool
    /// are built on.
    pub fn all_flops() -> impl Iterator<Item = Board> {
        (0..NUM_CARDS as u8).flat_map(move |a| {
            (a + 1..NUM_CARDS as u8).flat_map(move |b| {
                (b + 1..NUM_CARDS as u8).map(move |c| Board {
                    cards: [a, b, c, 0, 0],
                    len: 3,
                })
            })
        })
    }
}

impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for card in self.cards() {
            if !first {
                write!(f, " ")?;
            }
            write!(f, "{card}")?;
            first = false;
        }
        Ok(())
    }
}

impl fmt::Debug for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Board({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boards_read_and_write() {
        let board = Board::parse("Kc Qh Jh").unwrap();
        assert_eq!(board.len(), 3);
        assert_eq!(board.street(), Street::Flop);
        assert_eq!(board.to_string(), "Kc Qh Jh");
        assert_eq!(&*board.ranks_desc(), &[11u8, 10, 9]);
        assert_eq!(board.suit_counts(), [1, 0, 2, 0]);
    }

    #[test]
    fn duplicates_and_overflow_are_rejected() {
        assert!(Board::parse("Kc Kc Jh").is_err());
        assert!(Board::parse("Kc Qh Jh Ts 9d 8c").is_err());
    }

    #[test]
    fn streets_follow_the_card_count() {
        assert_eq!(Board::empty().street(), Street::Preflop);
        assert_eq!(Board::parse("Kc Qh Jh").unwrap().street(), Street::Flop);
        assert_eq!(Board::parse("Kc Qh Jh Ts").unwrap().street(), Street::Turn);
        assert_eq!(
            Board::parse("Kc Qh Jh Ts 2d").unwrap().street(),
            Street::River
        );
    }

    #[test]
    fn cards_can_be_pushed_and_popped() {
        let flop = Board::parse("Kc Qh Jh").unwrap();
        let turn = flop
            .with_card(crate::cards::Card::parse("Ts").unwrap())
            .unwrap();
        assert_eq!(turn.street(), Street::Turn);
        assert_eq!(turn.without_last(), flop);
        assert!(flop
            .with_card(crate::cards::Card::parse("Kc").unwrap())
            .is_none());
    }

    #[test]
    fn there_are_22100_flops() {
        assert_eq!(Board::all_flops().count(), 22_100);
    }
}
