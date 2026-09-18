//! A five-to-seven card Texas Hold'em hand evaluator.
//!
//! [`eval`] returns a [`HandRank`] that packs the hand category and up to five
//! ordered kickers into a single `u32`, so two hands compare with one integer
//! comparison. There are no lookup tables: the evaluator works straight off rank
//! counts and suit masks, which keeps the crate dependency-free and small enough
//! to stay fast in WebAssembly.

use crate::cards::{CardSet, RANK_ACE, RANK_FIVE};

/// The nine hand categories, ordered weakest to strongest.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[repr(u8)]
pub enum Category {
    /// No pair.
    HighCard = 0,
    /// Exactly one pair.
    OnePair = 1,
    /// Two pairs.
    TwoPair = 2,
    /// Three of a kind.
    Trips = 3,
    /// Five cards in sequence.
    Straight = 4,
    /// Five cards of one suit.
    Flush = 5,
    /// Three of a kind plus a pair.
    FullHouse = 6,
    /// Four of a kind.
    Quads = 7,
    /// A straight in one suit.
    StraightFlush = 8,
}

impl Category {
    /// A short lower-case name, as used in the statistics panel.
    pub const fn label(self) -> &'static str {
        match self {
            Self::HighCard => "high card",
            Self::OnePair => "one pair",
            Self::TwoPair => "two pair",
            Self::Trips => "three of a kind",
            Self::Straight => "straight",
            Self::Flush => "flush",
            Self::FullHouse => "full house",
            Self::Quads => "quads",
            Self::StraightFlush => "straight flush",
        }
    }
}

/// The strength of a five-card hand, comparable with `<` and `>`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct HandRank(u32);

impl HandRank {
    /// The weakest possible value, useful as a fold identity.
    pub const WORST: Self = Self(0);

    fn new(category: Category, kickers: [u8; 5]) -> Self {
        let mut value = (category as u32) << 20;
        for (i, kicker) in kickers.iter().enumerate() {
            value |= u32::from(*kicker) << (16 - 4 * i);
        }
        Self(value)
    }

    /// The packed value.
    pub const fn value(self) -> u32 {
        self.0
    }

    /// The hand's category.
    pub const fn category(self) -> Category {
        match (self.0 >> 20) as u8 {
            8 => Category::StraightFlush,
            7 => Category::Quads,
            6 => Category::FullHouse,
            5 => Category::Flush,
            4 => Category::Straight,
            3 => Category::Trips,
            2 => Category::TwoPair,
            1 => Category::OnePair,
            _ => Category::HighCard,
        }
    }

    /// The top kicker, which for most categories is the rank that names the hand.
    pub const fn primary_rank(self) -> u8 {
        ((self.0 >> 16) & 0xF) as u8
    }
}

/// Number of distinct 13-bit rank masks.
const RANK_MASKS: usize = 1 << 13;

/// `straight_high` for every rank mask, plus one: `0xFF` stands for "no straight".
///
/// Classifying a range against all 22,100 flops asks these questions about thirty
/// million times, and the window search underneath is a hundred-odd operations.
/// Sixteen kilobytes of table turns each one into a load.
static STRAIGHT_HIGH: [u8; RANK_MASKS] = build_straight_high();

/// `straight_outs` for every rank mask.
static STRAIGHT_OUTS: [u16; RANK_MASKS] = build_straight_outs();

const NO_STRAIGHT: u8 = 0xFF;

const fn scan_straight_high(rank_mask: u16) -> u8 {
    let mut high = RANK_ACE;
    while high >= 4 {
        let window = 0b11111u16 << (high - 4);
        if rank_mask & window == window {
            return high;
        }
        high -= 1;
    }
    const WHEEL: u16 = (1 << RANK_ACE) | 0b1111;
    if rank_mask & WHEEL == WHEEL {
        return RANK_FIVE;
    }
    NO_STRAIGHT
}

const fn build_straight_high() -> [u8; RANK_MASKS] {
    let mut table = [NO_STRAIGHT; RANK_MASKS];
    let mut mask = 0usize;
    while mask < RANK_MASKS {
        table[mask] = scan_straight_high(mask as u16);
        mask += 1;
    }
    table
}

const fn build_straight_outs() -> [u16; RANK_MASKS] {
    let mut table = [0u16; RANK_MASKS];
    let mut mask = 0usize;
    while mask < RANK_MASKS {
        if scan_straight_high(mask as u16) == NO_STRAIGHT {
            let mut outs = 0u16;
            let mut rank = 0u8;
            while rank < 13 {
                let bit = 1u16 << rank;
                if mask as u16 & bit == 0 && scan_straight_high(mask as u16 | bit) != NO_STRAIGHT {
                    outs |= bit;
                }
                rank += 1;
            }
            table[mask] = outs;
        }
        mask += 1;
    }
    table
}

/// The high rank of the best straight inside a 13-bit rank mask, if any.
///
/// The wheel (`A5432`) is reported as a five, matching how straights are ranked.
pub fn straight_high(rank_mask: u16) -> Option<u8> {
    match STRAIGHT_HIGH[rank_mask as usize] {
        NO_STRAIGHT => None,
        high => Some(high),
    }
}

/// The ranks that would complete a straight if added to `rank_mask`.
///
/// Returns an empty mask when the hand already holds a straight, so callers can
/// treat "has a draw" and "has the hand" as mutually exclusive.
pub fn straight_outs(rank_mask: u16) -> u16 {
    STRAIGHT_OUTS[rank_mask as usize]
}

fn top_ranks(mut mask: u16, count: usize) -> [u8; 5] {
    let mut out = [0u8; 5];
    let mut filled = 0;
    while filled < count && mask != 0 {
        let rank = 15 - mask.leading_zeros() as u8;
        out[filled] = rank;
        mask &= !(1u16 << rank);
        filled += 1;
    }
    out
}

fn top_ranks_excluding(mask: u16, exclude: &[u8], count: usize) -> [u8; 5] {
    let mut filtered = mask;
    for rank in exclude {
        filtered &= !(1u16 << rank);
    }
    top_ranks(filtered, count)
}

/// The category of the best hand inside `cards`, without ranking it.
///
/// The classifier only needs to know *what* a hand is, not which of two aces-up
/// hands is better, and skipping the kicker work makes the preflop pass over all
/// 22,100 flops several times cheaper.
///
/// # Panics
/// Panics if fewer than five cards are given.
pub fn category(cards: CardSet) -> Category {
    assert!(cards.len() >= 5, "a hand needs at least five cards");

    let mut rank_counts = [0u8; 13];
    let mut suit_masks = [0u16; 4];
    let mut rank_mask = 0u16;
    for card in cards.iter() {
        rank_counts[card.rank() as usize] += 1;
        suit_masks[card.suit() as usize] |= 1u16 << card.rank();
        rank_mask |= 1u16 << card.rank();
    }

    let flush = suit_masks.iter().find(|m| m.count_ones() >= 5);
    if let Some(mask) = flush {
        if straight_high(*mask).is_some() {
            return Category::StraightFlush;
        }
    }

    let mut quads = false;
    let mut trips = 0u8;
    let mut pairs = 0u8;
    for count in rank_counts {
        match count {
            4 => quads = true,
            3 => trips += 1,
            2 => pairs += 1,
            _ => {}
        }
    }

    if quads {
        return Category::Quads;
    }
    if trips >= 2 || (trips == 1 && pairs >= 1) {
        return Category::FullHouse;
    }
    if flush.is_some() {
        return Category::Flush;
    }
    if straight_high(rank_mask).is_some() {
        return Category::Straight;
    }
    if trips == 1 {
        return Category::Trips;
    }
    match pairs {
        0 => Category::HighCard,
        1 => Category::OnePair,
        _ => Category::TwoPair,
    }
}

/// Evaluates the best five-card hand inside `cards`.
///
/// # Panics
/// Panics if fewer than five cards are given.
pub fn eval(cards: CardSet) -> HandRank {
    assert!(cards.len() >= 5, "a hand needs at least five cards");

    let mut rank_counts = [0u8; 13];
    let mut suit_masks = [0u16; 4];
    let mut rank_mask = 0u16;
    for card in cards.iter() {
        rank_counts[card.rank() as usize] += 1;
        suit_masks[card.suit() as usize] |= 1u16 << card.rank();
        rank_mask |= 1u16 << card.rank();
    }

    let flush_suit = suit_masks.iter().position(|m| m.count_ones() >= 5);

    if let Some(suit) = flush_suit {
        if let Some(high) = straight_high(suit_masks[suit]) {
            return HandRank::new(Category::StraightFlush, [high, 0, 0, 0, 0]);
        }
    }

    // Seven cards hold at most one set of quads, two trips and three pairs, so
    // fixed arrays do: this runs tens of millions of times and must not allocate.
    let mut quads = None;
    let mut trips = [0u8; 3];
    let mut trip_count = 0usize;
    let mut pairs = [0u8; 3];
    let mut pair_count = 0usize;
    for rank in (0..13u8).rev() {
        match rank_counts[rank as usize] {
            4 => quads = quads.or(Some(rank)),
            3 => {
                trips[trip_count] = rank;
                trip_count += 1;
            }
            2 => {
                pairs[pair_count] = rank;
                pair_count += 1;
            }
            _ => {}
        }
    }
    let trips = &trips[..trip_count];
    let pairs = &pairs[..pair_count];

    if let Some(quad) = quads {
        let kickers = top_ranks_excluding(rank_mask, &[quad], 1);
        return HandRank::new(Category::Quads, [quad, kickers[0], 0, 0, 0]);
    }

    if let Some(&trip) = trips.first() {
        if let Some(pair) = trips.get(1).copied().or_else(|| pairs.first().copied()) {
            return HandRank::new(Category::FullHouse, [trip, pair, 0, 0, 0]);
        }
    }

    if let Some(suit) = flush_suit {
        return HandRank::new(Category::Flush, top_ranks(suit_masks[suit], 5));
    }

    if let Some(high) = straight_high(rank_mask) {
        return HandRank::new(Category::Straight, [high, 0, 0, 0, 0]);
    }

    if let Some(&trip) = trips.first() {
        let k = top_ranks_excluding(rank_mask, &[trip], 2);
        return HandRank::new(Category::Trips, [trip, k[0], k[1], 0, 0]);
    }

    if pairs.len() >= 2 {
        let (high, low) = (pairs[0], pairs[1]);
        let k = top_ranks_excluding(rank_mask, &[high, low], 1);
        return HandRank::new(Category::TwoPair, [high, low, k[0], 0, 0]);
    }

    if let Some(&pair) = pairs.first() {
        let k = top_ranks_excluding(rank_mask, &[pair], 3);
        return HandRank::new(Category::OnePair, [pair, k[0], k[1], k[2], 0]);
    }

    HandRank::new(Category::HighCard, top_ranks(rank_mask, 5))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::Card;

    fn rank_of(cards: &str) -> HandRank {
        eval(CardSet::parse(cards).unwrap())
    }

    fn category_of(cards: &str) -> Category {
        rank_of(cards).category()
    }

    #[test]
    fn the_fast_path_agrees_with_the_full_evaluator() {
        // Every five-card hand drawn from a reduced deck, so the cheap category
        // path can never disagree with the one that ranks kickers too.
        let deck: Vec<Card> = (0..24u8).map(Card::from_index).collect();
        let mut checked = 0;
        for a in 0..deck.len() {
            for b in a + 1..deck.len() {
                for c in b + 1..deck.len() {
                    for d in c + 1..deck.len() {
                        for e in d + 1..deck.len() {
                            let hand: CardSet = [deck[a], deck[b], deck[c], deck[d], deck[e]]
                                .into_iter()
                                .collect();
                            assert_eq!(category(hand), eval(hand).category(), "{hand}");
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 42_504);
    }

    #[test]
    fn categories_are_recognised() {
        assert_eq!(category_of("As Ks Qs Js Ts"), Category::StraightFlush);
        assert_eq!(category_of("5s 4s 3s 2s As"), Category::StraightFlush);
        assert_eq!(category_of("7c 7d 7h 7s 2c"), Category::Quads);
        assert_eq!(category_of("7c 7d 7h 2s 2c"), Category::FullHouse);
        assert_eq!(category_of("Ac Tc 8c 5c 2c"), Category::Flush);
        assert_eq!(category_of("9c 8d 7h 6s 5c"), Category::Straight);
        assert_eq!(category_of("Ac 5d 4h 3s 2c"), Category::Straight);
        assert_eq!(category_of("7c 7d 7h 9s 2c"), Category::Trips);
        assert_eq!(category_of("7c 7d 9h 9s 2c"), Category::TwoPair);
        assert_eq!(category_of("7c 7d 9h Js 2c"), Category::OnePair);
        assert_eq!(category_of("7c 9d Jh Ks 2c"), Category::HighCard);
    }

    #[test]
    fn the_wheel_is_the_weakest_straight() {
        assert!(rank_of("Ac 5d 4h 3s 2c") < rank_of("6c 5d 4h 3s 2c"));
        assert_eq!(rank_of("Ac 5d 4h 3s 2c").primary_rank(), RANK_FIVE);
    }

    #[test]
    fn seven_cards_pick_the_best_five() {
        // A flush is available alongside a straight; the flush must win.
        let seven = rank_of("Ac Kc Qc 9c 2c 3d 4h");
        assert_eq!(seven.category(), Category::Flush);
        // Board trips plus a pocket pair is a full house.
        assert_eq!(category_of("7c 7d 7h 2s 2c Kd 9h"), Category::FullHouse);
        // Two pair on board plus a higher pair in hand keeps the best two.
        let hand = rank_of("Ac Ad 9h 9s 2c 2d 5h");
        assert_eq!(hand.category(), Category::TwoPair);
        assert_eq!(hand.primary_rank(), Card::parse("Ac").unwrap().rank());
    }

    #[test]
    fn kickers_break_ties() {
        assert!(rank_of("Ac Ad Kh Qs 9c") > rank_of("Ac Ad Kh Js 9c"));
        assert!(rank_of("Ac Ad Kh Qs 9c") > rank_of("Kc Kd Ah Qs 9c"));
        assert_eq!(rank_of("Ac Ad Kh Qs 9c"), rank_of("As Ah Kd Qc 9d"));
    }

    #[test]
    fn category_order_is_total() {
        let ladder = [
            "2c 3d 5h 7s 9c",
            "2c 2d 5h 7s 9c",
            "2c 2d 5h 5s 9c",
            "2c 2d 2h 5s 9c",
            "5c 6d 7h 8s 9c",
            "2c 5c 7c 9c Jc",
            "2c 2d 2h 5s 5c",
            "2c 2d 2h 2s 5c",
            "5c 6c 7c 8c 9c",
        ];
        for pair in ladder.windows(2) {
            assert!(
                rank_of(pair[0]) < rank_of(pair[1]),
                "{} should be weaker than {}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn straight_outs_are_correct() {
        let mask = |cards: &str| {
            CardSet::parse(cards)
                .unwrap()
                .iter()
                .fold(0u16, |m, c| m | (1u16 << c.rank()))
        };
        // 9876 is open ended: a ten or a five completes it.
        let outs = straight_outs(mask("9c 8d 7h 6s"));
        assert_eq!(outs.count_ones(), 2);
        // 9875 needs a six only.
        let outs = straight_outs(mask("9c 8d 7h 5s"));
        assert_eq!(outs.count_ones(), 1);
        // A made straight is not a draw.
        assert_eq!(straight_outs(mask("9c 8d 7h 6s 5c")), 0);
        // A double gutshot (J9875) takes a ten or a six.
        let outs = straight_outs(mask("Jc 9d 8h 7s 5c"));
        assert_eq!(outs.count_ones(), 2);
        // Four ranks spread too thin are no draw at all.
        assert_eq!(straight_outs(mask("Jc 9d 7h 5s")), 0);
    }
}
