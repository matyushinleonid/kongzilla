//! Cards, card sets, combos and the 169 cells of the starting-hand matrix.
//!
//! The important type here is [`Combo`]: a `u16` in `0..1326` that identifies one
//! unordered pair of distinct cards. Every per-combo array in the engine is indexed
//! by it, which is what keeps the hot paths to flat array reads.

use crate::error::ParseError;
use core::fmt;

/// Number of distinct ranks (deuce through ace).
pub const NUM_RANKS: usize = 13;
/// Number of suits.
pub const NUM_SUITS: usize = 4;
/// Number of cards in a deck.
pub const NUM_CARDS: usize = 52;
/// Number of unordered two-card combinations.
pub const NUM_COMBOS: usize = 1326;
/// Number of cells in the 13x13 starting-hand matrix.
pub const NUM_CLASSES: usize = 169;

/// Rank characters, lowest rank first.
pub const RANK_CHARS: [u8; NUM_RANKS] = *b"23456789TJQKA";
/// Suit characters, in the canonical clubs/diamonds/hearts/spades order.
pub const SUIT_CHARS: [u8; NUM_SUITS] = *b"cdhs";

/// Rank index of the deuce.
pub const RANK_TWO: u8 = 0;
/// Rank index of the five, which is the high card of the wheel.
pub const RANK_FIVE: u8 = 3;
/// Rank index of the ace.
pub const RANK_ACE: u8 = 12;

/// A single card, stored as `rank * 4 + suit` in `0..52`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Card(u8);

impl Card {
    /// Builds a card from a rank in `0..13` and a suit in `0..4`.
    ///
    /// # Panics
    /// Panics if either index is out of range.
    pub const fn new(rank: u8, suit: u8) -> Self {
        assert!(rank < 13 && suit < 4, "rank or suit out of range");
        Self(rank * 4 + suit)
    }

    /// Builds a card from its deck index in `0..52`.
    ///
    /// # Panics
    /// Panics if the index is out of range.
    pub const fn from_index(index: u8) -> Self {
        assert!((index as usize) < NUM_CARDS, "card index out of range");
        Self(index)
    }

    /// The card's deck index in `0..52`.
    pub const fn index(self) -> u8 {
        self.0
    }

    /// The card's rank index, `0` for a deuce and `12` for an ace.
    pub const fn rank(self) -> u8 {
        self.0 >> 2
    }

    /// The card's suit index in `0..4`.
    pub const fn suit(self) -> u8 {
        self.0 & 3
    }

    /// A [`CardSet`] holding just this card.
    pub const fn mask(self) -> CardSet {
        CardSet(1u64 << self.0)
    }

    /// Reads a card such as `Kc`, `td` or `2H`. Case-insensitive.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let bytes = text.as_bytes();
        if bytes.len() != 2 {
            return Err(ParseError::BadCard(text.to_owned()));
        }
        let rank = RANK_CHARS
            .iter()
            .position(|&c| c == bytes[0].to_ascii_uppercase())
            .ok_or_else(|| ParseError::BadCard(text.to_owned()))?;
        let suit = SUIT_CHARS
            .iter()
            .position(|&c| c == bytes[1].to_ascii_lowercase())
            .ok_or_else(|| ParseError::BadCard(text.to_owned()))?;
        Ok(Self::new(rank as u8, suit as u8))
    }

    /// Every card in the deck, in deck-index order.
    pub fn all() -> impl Iterator<Item = Card> {
        (0..NUM_CARDS as u8).map(Card)
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            RANK_CHARS[self.rank() as usize] as char,
            SUIT_CHARS[self.suit() as usize] as char
        )
    }
}

impl fmt::Debug for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

/// A set of cards held as a 52-bit mask.
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct CardSet(u64);

impl CardSet {
    /// The empty set.
    pub const EMPTY: Self = Self(0);
    /// Every card in the deck.
    pub const FULL: Self = Self((1u64 << NUM_CARDS) - 1);

    /// Wraps a raw 52-bit mask.
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits & Self::FULL.0)
    }

    /// The raw 52-bit mask.
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// Number of cards in the set.
    pub const fn len(self) -> u32 {
        self.0.count_ones()
    }

    /// Whether the set holds no cards.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether `card` is in the set.
    pub const fn contains(self, card: Card) -> bool {
        self.0 & (1u64 << card.index()) != 0
    }

    /// Whether the two sets share at least one card.
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Adds a card.
    pub fn insert(&mut self, card: Card) {
        self.0 |= 1u64 << card.index();
    }

    /// Removes a card.
    pub fn remove(&mut self, card: Card) {
        self.0 &= !(1u64 << card.index());
    }

    /// Set union.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Set intersection.
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Everything in `self` that is not in `other`.
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Every card not in the set.
    pub const fn complement(self) -> Self {
        Self(!self.0 & Self::FULL.0)
    }

    /// Iterates the cards in deck-index order.
    pub fn iter(self) -> impl Iterator<Item = Card> {
        let mut bits = self.0;
        core::iter::from_fn(move || {
            if bits == 0 {
                None
            } else {
                let index = bits.trailing_zeros() as u8;
                bits &= bits - 1;
                Some(Card(index))
            }
        })
    }

    /// Reads a whitespace- or comma-separated list of cards such as `Kc Qh Jh`.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let mut set = Self::EMPTY;
        for token in text.split([' ', ',', '\t', '\n']).filter(|t| !t.is_empty()) {
            let card = Card::parse(token)?;
            if set.contains(card) {
                return Err(ParseError::DuplicateCard(token.to_owned()));
            }
            set.insert(card);
        }
        Ok(set)
    }
}

impl fmt::Display for CardSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for card in self.iter() {
            if !first {
                write!(f, " ")?;
            }
            write!(f, "{card}")?;
            first = false;
        }
        Ok(())
    }
}

impl fmt::Debug for CardSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CardSet({self})")
    }
}

impl FromIterator<Card> for CardSet {
    fn from_iter<I: IntoIterator<Item = Card>>(iter: I) -> Self {
        let mut set = Self::EMPTY;
        for card in iter {
            set.insert(card);
        }
        set
    }
}

/// Maps every combo index to its two card indices, low card first.
const COMBO_CARDS: [[u8; 2]; NUM_COMBOS] = {
    let mut table = [[0u8; 2]; NUM_COMBOS];
    let mut hi = 1usize;
    while hi < NUM_CARDS {
        let mut lo = 0usize;
        while lo < hi {
            table[hi * (hi - 1) / 2 + lo] = [lo as u8, hi as u8];
            lo += 1;
        }
        hi += 1;
    }
    table
};

/// One unordered pair of distinct cards, indexed `0..1326`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Combo(u16);

impl Combo {
    /// Builds a combo from two distinct cards, in either order.
    ///
    /// # Panics
    /// Panics if the two cards are the same.
    pub const fn new(a: Card, b: Card) -> Self {
        assert!(a.0 != b.0, "a combo needs two distinct cards");
        let (lo, hi) = if a.0 < b.0 {
            (a.0 as usize, b.0 as usize)
        } else {
            (b.0 as usize, a.0 as usize)
        };
        Self((hi * (hi - 1) / 2 + lo) as u16)
    }

    /// Builds a combo from its index in `0..1326`.
    ///
    /// # Panics
    /// Panics if the index is out of range.
    pub const fn from_index(index: u16) -> Self {
        assert!((index as usize) < NUM_COMBOS, "combo index out of range");
        Self(index)
    }

    /// The combo's index in `0..1326`.
    pub const fn index(self) -> u16 {
        self.0
    }

    /// The two cards, lower deck index first.
    pub const fn cards(self) -> (Card, Card) {
        let pair = COMBO_CARDS[self.0 as usize];
        (Card(pair[0]), Card(pair[1]))
    }

    /// The card with the higher rank; ties broken by deck index.
    pub const fn high(self) -> Card {
        let (a, b) = self.cards();
        if a.rank() >= b.rank() {
            a
        } else {
            b
        }
    }

    /// The card with the lower rank; ties broken by deck index.
    pub const fn low(self) -> Card {
        let (a, b) = self.cards();
        if a.rank() >= b.rank() {
            b
        } else {
            a
        }
    }

    /// A [`CardSet`] holding both cards.
    pub const fn mask(self) -> CardSet {
        let pair = COMBO_CARDS[self.0 as usize];
        CardSet((1u64 << pair[0]) | (1u64 << pair[1]))
    }

    /// Whether both cards share a suit.
    pub const fn is_suited(self) -> bool {
        let (a, b) = self.cards();
        a.suit() == b.suit()
    }

    /// Whether both cards share a rank.
    pub const fn is_pair(self) -> bool {
        let (a, b) = self.cards();
        a.rank() == b.rank()
    }

    /// The matrix cell this combo belongs to.
    pub const fn class(self) -> HandClass {
        let (a, b) = self.cards();
        HandClass::from_ranks(a.rank(), b.rank(), a.suit() == b.suit())
    }

    /// Reads a combo such as `AhKh`.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        if text.len() != 4 {
            return Err(ParseError::BadCard(text.to_owned()));
        }
        let a = Card::parse(&text[0..2])?;
        let b = Card::parse(&text[2..4])?;
        if a == b {
            return Err(ParseError::DuplicateCard(text.to_owned()));
        }
        Ok(Self::new(a, b))
    }

    /// Every combo, in index order.
    pub fn all() -> impl Iterator<Item = Combo> {
        (0..NUM_COMBOS as u16).map(Combo)
    }
}

impl fmt::Display for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.high(), self.low())
    }
}

impl fmt::Debug for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

/// One cell of the 13x13 starting-hand matrix: a pair, a suited hand or an
/// offsuit hand.
///
/// The index is `row * 13 + col` where both coordinates count down from the ace,
/// matching the on-screen matrix: `row < col` is suited (above the diagonal),
/// `row > col` is offsuit, `row == col` is a pocket pair.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HandClass(u8);

impl HandClass {
    /// Builds a class from matrix coordinates, each counting down from the ace.
    ///
    /// # Panics
    /// Panics if either coordinate is out of range.
    pub const fn from_cell(row: u8, col: u8) -> Self {
        assert!(row < 13 && col < 13, "matrix coordinate out of range");
        Self(row * 13 + col)
    }

    /// Builds a class from two ranks and a suitedness flag.
    ///
    /// # Panics
    /// Panics if a pocket pair is marked suited.
    pub const fn from_ranks(a: u8, b: u8, suited: bool) -> Self {
        let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
        assert!(!(suited && hi == lo), "a pocket pair cannot be suited");
        let hi_cell = RANK_ACE - hi;
        let lo_cell = RANK_ACE - lo;
        if suited {
            Self(hi_cell * 13 + lo_cell)
        } else {
            Self(lo_cell * 13 + hi_cell)
        }
    }

    /// Builds a class from its index in `0..169`.
    ///
    /// # Panics
    /// Panics if the index is out of range.
    pub const fn from_index(index: u8) -> Self {
        assert!((index as usize) < NUM_CLASSES, "class index out of range");
        Self(index)
    }

    /// The class index in `0..169`.
    pub const fn index(self) -> u8 {
        self.0
    }

    /// Matrix row, counting down from the ace.
    pub const fn row(self) -> u8 {
        self.0 / 13
    }

    /// Matrix column, counting down from the ace.
    pub const fn col(self) -> u8 {
        self.0 % 13
    }

    /// Whether this cell is a pocket pair.
    pub const fn is_pair(self) -> bool {
        self.row() == self.col()
    }

    /// Whether this cell is a suited hand.
    pub const fn is_suited(self) -> bool {
        self.row() < self.col()
    }

    /// The higher of the two ranks.
    pub const fn high_rank(self) -> u8 {
        let (row, col) = (self.row(), self.col());
        RANK_ACE - if row < col { row } else { col }
    }

    /// The lower of the two ranks.
    pub const fn low_rank(self) -> u8 {
        let (row, col) = (self.row(), self.col());
        RANK_ACE - if row < col { col } else { row }
    }

    /// How many combos this cell holds: 6 for a pair, 4 suited, 12 offsuit.
    pub const fn combo_count(self) -> u8 {
        if self.is_pair() {
            6
        } else if self.is_suited() {
            4
        } else {
            12
        }
    }

    /// The combos that belong to this cell.
    pub fn combos(self) -> impl Iterator<Item = Combo> {
        let (hi, lo, suited, pair) = (
            self.high_rank(),
            self.low_rank(),
            self.is_suited(),
            self.is_pair(),
        );
        (0..NUM_SUITS as u8).flat_map(move |sa| {
            (0..NUM_SUITS as u8).filter_map(move |sb| {
                let a = Card::new(hi, sa);
                let b = Card::new(lo, sb);
                let keep = if pair {
                    sa < sb
                } else if suited {
                    sa == sb
                } else {
                    sa != sb
                };
                if keep && a != b {
                    Some(Combo::new(a, b))
                } else {
                    None
                }
            })
        })
    }

    /// Reads a class such as `AA`, `AKs` or `AKo`.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let bytes = text.as_bytes();
        let bad = || ParseError::BadHandClass(text.to_owned());
        if bytes.len() < 2 || bytes.len() > 3 {
            return Err(bad());
        }
        let rank_of = |b: u8| {
            RANK_CHARS
                .iter()
                .position(|&c| c == b.to_ascii_uppercase())
                .map(|r| r as u8)
        };
        let hi = rank_of(bytes[0]).ok_or_else(bad)?;
        let lo = rank_of(bytes[1]).ok_or_else(bad)?;
        match (bytes.len(), hi == lo) {
            (2, true) => Ok(Self::from_ranks(hi, lo, false)),
            (3, false) => match bytes[2].to_ascii_lowercase() {
                b's' => Ok(Self::from_ranks(hi, lo, true)),
                b'o' => Ok(Self::from_ranks(hi, lo, false)),
                _ => Err(bad()),
            },
            _ => Err(bad()),
        }
    }

    /// Every class, in matrix index order.
    pub fn all() -> impl Iterator<Item = HandClass> {
        (0..NUM_CLASSES as u8).map(HandClass)
    }
}

impl fmt::Display for HandClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hi = RANK_CHARS[self.high_rank() as usize] as char;
        let lo = RANK_CHARS[self.low_rank() as usize] as char;
        if self.is_pair() {
            write!(f, "{hi}{lo}")
        } else if self.is_suited() {
            write!(f, "{hi}{lo}s")
        } else {
            write!(f, "{hi}{lo}o")
        }
    }
}

impl fmt::Debug for HandClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

/// Number of `u64` words needed to hold one bit per combo.
pub const COMBO_WORDS: usize = NUM_COMBOS.div_ceil(64);

/// A set of combos held as a 1326-bit mask.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ComboSet([u64; COMBO_WORDS]);

impl ComboSet {
    /// The empty set.
    pub const EMPTY: Self = Self([0; COMBO_WORDS]);

    /// Whether `combo` is in the set.
    pub const fn contains(&self, combo: Combo) -> bool {
        let i = combo.0 as usize;
        self.0[i / 64] & (1u64 << (i % 64)) != 0
    }

    /// Adds a combo.
    pub fn insert(&mut self, combo: Combo) {
        let i = combo.0 as usize;
        self.0[i / 64] |= 1u64 << (i % 64);
    }

    /// Removes a combo.
    pub fn remove(&mut self, combo: Combo) {
        let i = combo.0 as usize;
        self.0[i / 64] &= !(1u64 << (i % 64));
    }

    /// Number of combos in the set.
    pub fn len(&self) -> u32 {
        self.0.iter().map(|w| w.count_ones()).sum()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|&w| w == 0)
    }

    /// Set intersection.
    pub fn intersection(&self, other: &Self) -> Self {
        let mut out = Self::EMPTY;
        for i in 0..COMBO_WORDS {
            out.0[i] = self.0[i] & other.0[i];
        }
        out
    }

    /// Set union.
    pub fn union(&self, other: &Self) -> Self {
        let mut out = Self::EMPTY;
        for i in 0..COMBO_WORDS {
            out.0[i] = self.0[i] | other.0[i];
        }
        out
    }

    /// Iterates the combos in index order.
    pub fn iter(&self) -> impl Iterator<Item = Combo> + '_ {
        (0..COMBO_WORDS).flat_map(move |word| {
            let mut bits = self.0[word];
            core::iter::from_fn(move || {
                if bits == 0 {
                    None
                } else {
                    let bit = bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    Some(Combo((word * 64 + bit) as u16))
                }
            })
        })
    }
}

impl Default for ComboSet {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl fmt::Debug for ComboSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ComboSet({} combos)", self.len())
    }
}

impl FromIterator<Combo> for ComboSet {
    fn from_iter<I: IntoIterator<Item = Combo>>(iter: I) -> Self {
        let mut set = Self::EMPTY;
        for combo in iter {
            set.insert(combo);
        }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_round_trips_through_text() {
        for card in Card::all() {
            assert_eq!(Card::parse(&card.to_string()).unwrap(), card);
        }
        assert_eq!(Card::parse("kC").unwrap(), Card::parse("Kc").unwrap());
        assert!(Card::parse("Xc").is_err());
        assert!(Card::parse("K").is_err());
    }

    #[test]
    fn combo_indices_are_a_bijection() {
        let mut seen = vec![false; NUM_COMBOS];
        let mut count = 0;
        for a in Card::all() {
            for b in Card::all() {
                if a.index() >= b.index() {
                    continue;
                }
                let combo = Combo::new(a, b);
                assert!(!seen[combo.index() as usize]);
                seen[combo.index() as usize] = true;
                assert_eq!(combo.cards(), (a, b));
                assert_eq!(Combo::new(b, a), combo);
                count += 1;
            }
        }
        assert_eq!(count, NUM_COMBOS);
        assert!(seen.into_iter().all(|s| s));
    }

    #[test]
    fn classes_partition_the_combos() {
        let mut counts = [0u32; NUM_CLASSES];
        for combo in Combo::all() {
            counts[combo.class().index() as usize] += 1;
        }
        for class in HandClass::all() {
            assert_eq!(
                counts[class.index() as usize],
                u32::from(class.combo_count()),
                "wrong combo count for {class}"
            );
            let listed: Vec<_> = class.combos().collect();
            assert_eq!(listed.len(), usize::from(class.combo_count()));
            for combo in listed {
                assert_eq!(combo.class(), class, "{combo} does not belong to {class}");
            }
        }
        assert_eq!(counts.iter().sum::<u32>(), NUM_COMBOS as u32);
    }

    #[test]
    fn class_text_round_trips() {
        for class in HandClass::all() {
            assert_eq!(HandClass::parse(&class.to_string()).unwrap(), class);
        }
        assert_eq!(HandClass::parse("AKs").unwrap().to_string(), "AKs");
        assert_eq!(HandClass::parse("KAs").unwrap().to_string(), "AKs");
        assert_eq!(HandClass::parse("77").unwrap().to_string(), "77");
        assert!(HandClass::parse("AAs").is_err());
        assert!(HandClass::parse("AKx").is_err());
    }

    #[test]
    fn matrix_corners_are_where_flopzilla_puts_them() {
        assert_eq!(HandClass::from_cell(0, 0).to_string(), "AA");
        assert_eq!(HandClass::from_cell(0, 1).to_string(), "AKs");
        assert_eq!(HandClass::from_cell(1, 0).to_string(), "AKo");
        assert_eq!(HandClass::from_cell(12, 12).to_string(), "22");
        assert_eq!(HandClass::from_cell(0, 12).to_string(), "A2s");
        assert_eq!(HandClass::from_cell(12, 0).to_string(), "A2o");
    }

    #[test]
    fn card_sets_read_and_write() {
        let set = CardSet::parse("Kc Qh Jh").unwrap();
        assert_eq!(set.len(), 3);
        assert!(set.contains(Card::parse("Qh").unwrap()));
        assert_eq!(set.to_string(), "Jh Qh Kc", "sets iterate in deck order");
        assert!(CardSet::parse("Kc Kc").is_err());
    }

    #[test]
    fn combo_sets_round_trip() {
        let combos: Vec<_> = HandClass::parse("AKs").unwrap().combos().collect();
        let set: ComboSet = combos.iter().copied().collect();
        assert_eq!(set.len(), 4);
        assert_eq!(set.iter().count(), 4);
        for combo in &combos {
            assert!(set.contains(*combo));
        }
    }
}
