//! The statistic registry and the classifier that fills it.
//!
//! # Deliberate double counting
//!
//! Flopzilla computes made hands, draws and combinations **independently**, and so
//! do we. `AhKh` on `KcQhJh` is top pair in the made block, a flushdraw in the draw
//! block *and* `flushdraw+pair` in the combination block. A combo's classification
//! is therefore a [`StatMask`] - a bitset - rather than a single category. Every
//! downstream feature falls out of that bitset: filtering is a two-pass fold, the
//! overlap between two statistics is an `AND`, and a Pro-style custom statistic is
//! a boolean expression over the same bits.
//!
//! # Adding a statistic
//!
//! Append a [`StatDef`] to [`DEFS`], give it the next id, and teach [`classify`]
//! to set the bit. Nothing in the interface needs to change: the panels render
//! whatever the registry reports.

use crate::board::{Board, RankList};
use crate::cards::{CardSet, Combo, ComboSet, NUM_COMBOS, RANK_ACE};
use crate::eval::{category, eval, straight_outs, Category};
use core::fmt;

/// Which of the three coloured blocks a statistic belongs to.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum StatBlock {
    /// Completed hands, drawn in blue.
    Made,
    /// Hands that can still improve, drawn in green.
    Draw,
    /// A made hand held together with a draw, drawn in magenta.
    Combination,
}

impl StatBlock {
    /// Every block, in display order.
    pub const ALL: [StatBlock; 3] = [Self::Made, Self::Draw, Self::Combination];

    /// A stable identifier for serialisation.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Made => "made",
            Self::Draw => "draw",
            Self::Combination => "combination",
        }
    }

    /// The heading shown above the block.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Made => "Made hands",
            Self::Draw => "Draws",
            Self::Combination => "Combinations",
        }
    }
}

/// The index of a statistic in [`DEFS`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StatId(u8);

macro_rules! stat_ids {
    ($($konst:ident = $index:literal),* $(,)?) => {
        impl StatId {
            $(
                #[allow(missing_docs)]
                pub const $konst: StatId = StatId($index);
            )*
        }
    };
}

stat_ids! {
    STRAIGHT_FLUSH = 0,
    QUADS = 1,
    FULL_HOUSE = 2,
    FLUSH = 3,
    STRAIGHT = 4,
    SET = 5,
    TRIPS = 6,
    TWO_PAIR = 7,
    OVERPAIR = 8,
    TOP_PAIR = 9,
    PP_BELOW_TOP_CARD = 10,
    SECOND_PAIR = 11,
    PP_BELOW_SECOND_CARD = 12,
    BOTTOM_PAIR = 13,
    PP_BELOW_BOARD = 14,
    ACE_HIGH = 15,
    NO_MADE_HAND = 16,
    FLUSH_DRAW = 17,
    OESD_TWO_CARD = 18,
    OESD_ONE_CARD = 19,
    GUTSHOT_TWO_CARD = 20,
    GUTSHOT_ONE_CARD = 21,
    OVERCARDS = 22,
    BACKDOOR_FLUSH_DRAW_2 = 23,
    BACKDOOR_FLUSH_DRAW_1_HIGH = 24,
    BACKDOOR_FLUSH_DRAW_1_LOW = 25,
    FLUSH_DRAW_PLUS_PAIR = 26,
    FLUSH_DRAW_PLUS_OESD = 27,
    FLUSH_DRAW_PLUS_GUTSHOT = 28,
    FLUSH_DRAW_PLUS_OVERCARDS = 29,
    OESD_PLUS_PAIR = 30,
    GUTSHOT_PLUS_PAIR = 31,
    GUTSHOT_PLUS_OVERCARDS = 32,
    // Appended rather than slotted in beside the other pairs, because a shared
    // link writes a painted category as its index: put these where they are
    // read and every link made before today would come back painted wrong.
    // Where they are *shown* is `ORDER` below, which is a separate question.
    THIRD_PAIR = 33,
    FOURTH_PAIR = 34,
}

impl StatId {
    /// The statistic's index.
    pub const fn index(self) -> u8 {
        self.0
    }

    /// Builds a statistic id from an index, if one is registered.
    pub fn from_index(index: u8) -> Option<Self> {
        (usize::from(index) < DEFS.len()).then_some(Self(index))
    }

    /// The registry entry.
    pub fn def(self) -> &'static StatDef {
        &DEFS[self.0 as usize]
    }

    /// A single-bit mask holding just this statistic.
    pub const fn mask(self) -> StatMask {
        StatMask(1u128 << self.0)
    }

    /// Every registered statistic, in display order.
    ///
    /// Which is not index order: an index is a place in a saved link and may
    /// never move, while a place in the ladder is a matter of what reads well
    /// and has already had to change once. See [`ORDER`].
    pub fn all() -> impl Iterator<Item = StatId> {
        ORDER.iter().copied()
    }
}

impl fmt::Debug for StatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.def().key)
    }
}

/// A registry entry describing one statistic.
#[derive(Clone, Copy, Debug)]
pub struct StatDef {
    /// The statistic's index in the registry.
    pub id: StatId,
    /// A stable identifier for serialisation and saved filters.
    pub key: &'static str,
    /// The name shown in the statistics panel.
    pub label: &'static str,
    /// Which coloured block it belongs to.
    pub block: StatBlock,
    /// Whether the statistic is hidden unless a setting turns it on.
    pub optional: bool,
}

const fn def(
    id: StatId,
    key: &'static str,
    label: &'static str,
    block: StatBlock,
    optional: bool,
) -> StatDef {
    StatDef {
        id,
        key,
        label,
        block,
        optional,
    }
}

/// Every statistic, in the order the panel draws them.
///
/// The ladder, the labels and their order follow FlopzillaPro's own statistics
/// panel, so anyone arriving from that tool reads the same rows in the same
/// places. `ace high` is ours: Flopzilla v1 reported it and the Pro default set
/// folds it into `no made hand`, but the distinction is worth keeping.
pub static DEFS: &[StatDef] = &[
    def(
        StatId::STRAIGHT_FLUSH,
        "straight-flush",
        "straight flush",
        StatBlock::Made,
        false,
    ),
    def(StatId::QUADS, "quads", "quads", StatBlock::Made, false),
    def(
        StatId::FULL_HOUSE,
        "full-house",
        "full house",
        StatBlock::Made,
        false,
    ),
    def(StatId::FLUSH, "flush", "flush", StatBlock::Made, false),
    def(
        StatId::STRAIGHT,
        "straight",
        "straight",
        StatBlock::Made,
        false,
    ),
    def(StatId::SET, "set", "set", StatBlock::Made, false),
    def(StatId::TRIPS, "trips", "trips", StatBlock::Made, false),
    def(
        StatId::TWO_PAIR,
        "two-pair",
        "two pair",
        StatBlock::Made,
        false,
    ),
    def(
        StatId::OVERPAIR,
        "overpair",
        "overpair",
        StatBlock::Made,
        false,
    ),
    def(
        StatId::TOP_PAIR,
        "top-pair",
        "top pair",
        StatBlock::Made,
        false,
    ),
    def(
        StatId::PP_BELOW_TOP_CARD,
        "pp-below-top-card",
        "pp < top card",
        StatBlock::Made,
        false,
    ),
    def(
        StatId::SECOND_PAIR,
        "second-pair",
        "second pair",
        StatBlock::Made,
        false,
    ),
    def(
        StatId::PP_BELOW_SECOND_CARD,
        "pp-below-2nd-card",
        "pp < 2nd card",
        StatBlock::Made,
        false,
    ),
    def(
        StatId::BOTTOM_PAIR,
        "bottom-pair",
        "bottom pair",
        StatBlock::Made,
        false,
    ),
    def(
        StatId::PP_BELOW_BOARD,
        "pp-below-board",
        "pp < board",
        StatBlock::Made,
        false,
    ),
    def(
        StatId::ACE_HIGH,
        "ace-high",
        "ace high",
        StatBlock::Made,
        false,
    ),
    def(
        StatId::NO_MADE_HAND,
        "no-made-hand",
        "no made hand",
        StatBlock::Made,
        false,
    ),
    def(
        StatId::FLUSH_DRAW,
        "flushdraw",
        "flushdraw",
        StatBlock::Draw,
        false,
    ),
    def(
        StatId::OESD_TWO_CARD,
        "oesd-2",
        "oesd (2 card)",
        StatBlock::Draw,
        false,
    ),
    def(
        StatId::OESD_ONE_CARD,
        "oesd-1",
        "oesd (1 card)",
        StatBlock::Draw,
        false,
    ),
    def(
        StatId::GUTSHOT_TWO_CARD,
        "gutshot-2",
        "gutshot (2 crd)",
        StatBlock::Draw,
        false,
    ),
    def(
        StatId::GUTSHOT_ONE_CARD,
        "gutshot-1",
        "gutshot (1 crd)",
        StatBlock::Draw,
        false,
    ),
    def(
        StatId::OVERCARDS,
        "overcards",
        "overcards",
        StatBlock::Draw,
        false,
    ),
    def(
        StatId::BACKDOOR_FLUSH_DRAW_2,
        "bdfd-2",
        "2 crd bckdr fd",
        StatBlock::Draw,
        false,
    ),
    def(
        StatId::BACKDOOR_FLUSH_DRAW_1_HIGH,
        "bdfd-1-high",
        "1 crd bdfd high",
        StatBlock::Draw,
        true,
    ),
    def(
        StatId::BACKDOOR_FLUSH_DRAW_1_LOW,
        "bdfd-1-low",
        "1 crd bdfd low",
        StatBlock::Draw,
        true,
    ),
    def(
        StatId::FLUSH_DRAW_PLUS_PAIR,
        "fd-pair",
        "flushdraw+pair",
        StatBlock::Combination,
        false,
    ),
    def(
        StatId::FLUSH_DRAW_PLUS_OESD,
        "fd-oesd",
        "flushdr.+oesd",
        StatBlock::Combination,
        false,
    ),
    def(
        StatId::FLUSH_DRAW_PLUS_GUTSHOT,
        "fd-gutshot",
        "flushdr.+gutsh.",
        StatBlock::Combination,
        false,
    ),
    def(
        StatId::FLUSH_DRAW_PLUS_OVERCARDS,
        "fd-overcards",
        "flushdr.+overc.",
        StatBlock::Combination,
        false,
    ),
    def(
        StatId::OESD_PLUS_PAIR,
        "oesd-pair",
        "oesd+pair",
        StatBlock::Combination,
        false,
    ),
    def(
        StatId::GUTSHOT_PLUS_PAIR,
        "gutshot-pair",
        "gutshot+pair",
        StatBlock::Combination,
        false,
    ),
    def(
        StatId::GUTSHOT_PLUS_OVERCARDS,
        "gutshot-overcards",
        "gutshot+overc.",
        StatBlock::Combination,
        false,
    ),
    def(
        StatId::THIRD_PAIR,
        "third-pair",
        "third pair",
        StatBlock::Made,
        false,
    ),
    def(
        StatId::FOURTH_PAIR,
        "fourth-pair",
        "fourth pair",
        StatBlock::Made,
        false,
    ),
];

/// How many statistics the registry holds.
pub fn stat_count() -> usize {
    DEFS.len()
}

/// The ladder, top to bottom.
///
/// Separate from the registry because the two answer different questions: an
/// index is where a statistic lives in a saved link and may never move, and
/// this is where it reads best. The pairs below the top card are named by
/// which board card they hit - second, third, fourth - and the lowest is the
/// bottom pair whatever its number, so no two rungs can ever mean one thing.
pub const ORDER: [StatId; 35] = [
    StatId::STRAIGHT_FLUSH,
    StatId::QUADS,
    StatId::FULL_HOUSE,
    StatId::FLUSH,
    StatId::STRAIGHT,
    StatId::SET,
    StatId::TRIPS,
    StatId::TWO_PAIR,
    StatId::OVERPAIR,
    StatId::TOP_PAIR,
    StatId::PP_BELOW_TOP_CARD,
    StatId::SECOND_PAIR,
    StatId::PP_BELOW_SECOND_CARD,
    StatId::THIRD_PAIR,
    StatId::FOURTH_PAIR,
    StatId::BOTTOM_PAIR,
    StatId::PP_BELOW_BOARD,
    StatId::ACE_HIGH,
    StatId::NO_MADE_HAND,
    StatId::FLUSH_DRAW,
    StatId::OESD_TWO_CARD,
    StatId::OESD_ONE_CARD,
    StatId::GUTSHOT_TWO_CARD,
    StatId::GUTSHOT_ONE_CARD,
    StatId::OVERCARDS,
    StatId::BACKDOOR_FLUSH_DRAW_2,
    StatId::BACKDOOR_FLUSH_DRAW_1_HIGH,
    StatId::BACKDOOR_FLUSH_DRAW_1_LOW,
    StatId::FLUSH_DRAW_PLUS_PAIR,
    StatId::FLUSH_DRAW_PLUS_OESD,
    StatId::FLUSH_DRAW_PLUS_GUTSHOT,
    StatId::FLUSH_DRAW_PLUS_OVERCARDS,
    StatId::OESD_PLUS_PAIR,
    StatId::GUTSHOT_PLUS_PAIR,
    StatId::GUTSHOT_PLUS_OVERCARDS,
];

/// Looks a statistic up by its stable key.
pub fn stat_by_key(key: &str) -> Option<StatId> {
    DEFS.iter().find(|d| d.key == key).map(|d| d.id)
}

/// A set of statistics, one bit each.
///
/// 128 bits leaves room for roughly ninety more statistics before the
/// representation has to change, which is more than the full Flopzilla set plus a
/// generous allowance for user-defined ones.
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct StatMask(u128);

impl StatMask {
    /// No statistics.
    pub const EMPTY: Self = Self(0);

    /// Wraps a raw bitset.
    pub const fn from_bits(bits: u128) -> Self {
        Self(bits)
    }

    /// The raw bitset.
    pub const fn bits(self) -> u128 {
        self.0
    }

    /// Whether the statistic is present.
    pub const fn has(self, stat: StatId) -> bool {
        self.0 & (1u128 << stat.0) != 0
    }

    /// Whether any statistic in `other` is present.
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Adds a statistic.
    pub fn set(&mut self, stat: StatId) {
        self.0 |= 1u128 << stat.0;
    }

    /// Removes a statistic.
    pub fn clear(&mut self, stat: StatId) {
        self.0 &= !(1u128 << stat.0);
    }

    /// Whether no statistic is present.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Set union.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Set intersection.
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// How many statistics are present.
    pub const fn len(self) -> u32 {
        self.0.count_ones()
    }

    /// Iterates the statistics in registry order.
    pub fn iter(self) -> impl Iterator<Item = StatId> {
        let mut bits = self.0;
        core::iter::from_fn(move || {
            if bits == 0 {
                None
            } else {
                let index = bits.trailing_zeros() as u8;
                bits &= bits - 1;
                Some(StatId(index))
            }
        })
    }
}

impl fmt::Debug for StatMask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.iter().map(|s| s.def().key).collect();
        write!(f, "StatMask[{}]", names.join(" "))
    }
}

impl FromIterator<StatId> for StatMask {
    fn from_iter<I: IntoIterator<Item = StatId>>(iter: I) -> Self {
        let mut mask = Self::EMPTY;
        for stat in iter {
            mask.set(stat);
        }
        mask
    }
}

/// The one-pair rungs, from `overpair` down to `pp < board`.
///
/// A hand on any of them holds exactly one pair, which is what `+pair` means in
/// the combination block.
pub const PAIR_STATS: StatMask = StatMask(
    (1u128 << 8)
        | (1u128 << 9)
        | (1u128 << 10)
        | (1u128 << 11)
        | (1u128 << 12)
        | (1u128 << 13)
        | (1u128 << 14),
);

/// Both open-ended straight draw rungs.
pub const OESD_STATS: StatMask = StatMask((1u128 << 18) | (1u128 << 19));

/// Both gutshot rungs.
pub const GUTSHOT_STATS: StatMask = StatMask((1u128 << 20) | (1u128 << 21));

/// Settings that change how hands are classified.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, rename_all = "camelCase"))]
pub struct ClassifyOptions {
    /// Report one-card backdoor flushdraws on two-flush flops.
    ///
    /// Off by default, matching Flopzilla's `Settings -> One crd bckdr flushdraw
    /// on 2flush flops`.
    pub one_card_backdoor_flushdraw: bool,
}

impl ClassifyOptions {
    /// Whether a statistic is shown under these options.
    pub fn shows(&self, stat: StatId) -> bool {
        if !stat.def().optional {
            return true;
        }
        match stat {
            StatId::BACKDOOR_FLUSH_DRAW_1_HIGH | StatId::BACKDOOR_FLUSH_DRAW_1_LOW => {
                self.one_card_backdoor_flushdraw
            }
            _ => true,
        }
    }

    /// The statistics visible under these options, in display order.
    pub fn visible_stats(&self) -> Vec<StatId> {
        StatId::all().filter(|s| self.shows(*s)).collect()
    }
}

/// Every combo's classification against one board: the cache the whole engine
/// reads from.
///
/// Building this is the only pass that touches the evaluator. The statistics
/// panel, hover highlighting, the overlap between statistics and the filters are
/// all bit operations on top of it, which is what makes "no compute button"
/// achievable.
#[derive(Clone)]
pub struct ComboStats {
    board: Board,
    dead: CardSet,
    options: ClassifyOptions,
    masks: Box<[StatMask; NUM_COMBOS]>,
    live: ComboSet,
}

impl ComboStats {
    /// Classifies every combo against `board`, excluding anything that clashes
    /// with the board or with `dead`.
    pub fn build(board: &Board, dead: CardSet, options: ClassifyOptions) -> Self {
        let blocked = board.mask().union(dead);
        let context = BoardContext::new(board, options);
        let mut masks = Box::new([StatMask::EMPTY; NUM_COMBOS]);
        let mut live = ComboSet::EMPTY;
        for combo in Combo::all() {
            if combo.mask().intersects(blocked) {
                continue;
            }
            live.insert(combo);
            masks[combo.index() as usize] = context.classify(combo);
        }
        Self {
            board: *board,
            dead,
            options,
            masks,
            live,
        }
    }

    /// The board these statistics were built against.
    pub fn board(&self) -> &Board {
        &self.board
    }

    /// The dead cards excluded from the deck.
    pub fn dead(&self) -> CardSet {
        self.dead
    }

    /// The options used.
    pub fn options(&self) -> ClassifyOptions {
        self.options
    }

    /// The combos that are not blocked by the board or the dead cards.
    pub fn live(&self) -> &ComboSet {
        &self.live
    }

    /// One combo's classification. Blocked combos classify as empty.
    pub fn mask(&self, combo: Combo) -> StatMask {
        self.masks[combo.index() as usize]
    }

    /// Whether a combo is playable against this board.
    pub fn is_live(&self, combo: Combo) -> bool {
        self.live.contains(combo)
    }

    /// The combos that carry `stat`.
    pub fn combos_with(&self, stat: StatId) -> ComboSet {
        self.matching(stat.mask())
    }

    /// The combos that carry every statistic in `mask`.
    pub fn matching(&self, mask: StatMask) -> ComboSet {
        let mut set = ComboSet::EMPTY;
        for combo in self.live.iter() {
            if self.mask(combo).intersection(mask) == mask {
                set.insert(combo);
            }
        }
        set
    }
}

impl fmt::Debug for ComboStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ComboStats(board={}, live={})",
            self.board,
            self.live.len()
        )
    }
}

/// Everything about a board that does not change from hand to hand.
///
/// Classifying a range against one board recomputes these once instead of once
/// per combo; the preflop pass over all 22,100 flops recomputes them once per
/// flop instead of thirty million times.
#[derive(Clone, Debug)]
pub struct BoardContext {
    board: Board,
    mask: CardSet,
    rank_counts: [u8; 13],
    ranks: RankList,
    rank_mask: u16,
    board_outs: u16,
    suit_counts: [u8; 4],
    board_rank: Option<crate::eval::HandRank>,
    options: ClassifyOptions,
}

impl BoardContext {
    /// Precomputes everything the classifier needs about `board`.
    pub fn new(board: &Board, options: ClassifyOptions) -> Self {
        let mask = board.mask();
        let rank_mask = board.rank_mask();
        Self {
            board: *board,
            mask,
            rank_counts: board.rank_counts(),
            ranks: board.ranks_desc(),
            rank_mask,
            board_outs: straight_outs(rank_mask),
            suit_counts: board.suit_counts(),
            board_rank: (board.len() == Board::MAX).then(|| eval(mask)),
            options,
        }
    }

    /// The board this context describes.
    pub fn board(&self) -> &Board {
        &self.board
    }

    /// Classifies one combo. Returns an empty mask preflop, or when the combo
    /// clashes with the board.
    pub fn classify(&self, combo: Combo) -> StatMask {
        let mut mask = StatMask::EMPTY;
        if self.board.len() < 3 || combo.mask().intersects(self.mask) {
            return mask;
        }

        let made = self.made_stat(combo);
        mask.set(made);

        if self.board.len() < Board::MAX {
            self.add_draws(&mut mask, combo, made);
        }
        add_combinations(&mut mask);
        mask
    }

    /// Which rung of the made-hand ladder a combo sits on.
    ///
    /// A category only counts when the hole cards take part in it: a pair that
    /// lives entirely on the board leaves the hand on the ace-high or
    /// no-made-hand rung, and on the river a hand that cannot beat the board
    /// plays the board.
    fn made_stat(&self, combo: Combo) -> StatId {
        let full = combo.mask().union(self.mask);
        let category = match self.board_rank {
            Some(board_rank) => {
                let rank = eval(full);
                if board_rank >= rank {
                    return no_pair_rung(combo);
                }
                rank.category()
            }
            // Before the river the board cannot make a hand on its own, so the
            // cheap category path is enough.
            None => category(full),
        };

        let (a, b) = combo.cards();
        let is_pocket_pair = a.rank() == b.rank();

        match category {
            Category::StraightFlush => StatId::STRAIGHT_FLUSH,
            Category::Quads => StatId::QUADS,
            Category::FullHouse => StatId::FULL_HOUSE,
            Category::Flush => StatId::FLUSH,
            Category::Straight => StatId::STRAIGHT,
            Category::Trips => {
                if is_pocket_pair && self.rank_counts[a.rank() as usize] == 1 {
                    StatId::SET
                } else if !is_pocket_pair
                    && (self.rank_counts[a.rank() as usize] == 2
                        || self.rank_counts[b.rank() as usize] == 2)
                {
                    StatId::TRIPS
                } else {
                    // Trips sit entirely on the board.
                    no_pair_rung(combo)
                }
            }
            Category::TwoPair => match self.hole_pair_count(combo) {
                0 => no_pair_rung(combo),
                1 => self.pair_rung(combo),
                _ => StatId::TWO_PAIR,
            },
            Category::OnePair => {
                if self.hole_pair_count(combo) == 0 {
                    no_pair_rung(combo)
                } else {
                    self.pair_rung(combo)
                }
            }
            Category::HighCard => no_pair_rung(combo),
        }
    }

    /// How many pairs the hole cards take part in.
    fn hole_pair_count(&self, combo: Combo) -> u8 {
        let (a, b) = combo.cards();
        if a.rank() == b.rank() {
            return 1;
        }
        u8::from(self.rank_counts[a.rank() as usize] > 0)
            + u8::from(self.rank_counts[b.rank() as usize] > 0)
    }

    /// The pair ladder, for a hand whose hole cards make exactly one pair.
    ///
    /// Flopzilla interleaves pocket pairs with the board rungs, so the ladder
    /// reads `overpair, top pair, pp < top card, middle pair, pp < 2nd card,
    /// bottom pair, pp < board`. A hand that pairs the board is placed by which
    /// board rank it paired; a pocket pair by how many board ranks sit above it.
    fn pair_rung(&self, combo: Combo) -> StatId {
        let (a, b) = combo.cards();
        if a.rank() == b.rank() {
            return match self.ranks.iter().filter(|r| **r > a.rank()).count() {
                0 => StatId::OVERPAIR,
                1 => StatId::PP_BELOW_TOP_CARD,
                2 => StatId::PP_BELOW_SECOND_CARD,
                _ => StatId::PP_BELOW_BOARD,
            };
        }
        let paired = if self.rank_counts[a.rank() as usize] > 0 {
            a.rank()
        } else {
            b.rank()
        };
        // Which board card was hit, named by where it sits. The lowest one is
        // the bottom pair whatever its number, so "third pair" and "bottom
        // pair" can never be two names for the same thing - on a board of
        // three ranks there is no third pair at all, only a bottom one.
        let at = self.ranks.iter().position(|r| *r == paired);
        let lowest = self.ranks.len().saturating_sub(1);
        match at {
            Some(0) => StatId::TOP_PAIR,
            Some(place) if place == lowest => StatId::BOTTOM_PAIR,
            Some(1) => StatId::SECOND_PAIR,
            Some(2) => StatId::THIRD_PAIR,
            Some(3) => StatId::FOURTH_PAIR,
            _ => StatId::BOTTOM_PAIR,
        }
    }

    fn add_draws(&self, mask: &mut StatMask, combo: Combo, made: StatId) {
        let (a, b) = combo.cards();

        let mut suits_all = self.suit_counts;
        let mut suits_hole = [0u8; 4];
        for card in [a, b] {
            suits_all[card.suit() as usize] += 1;
            suits_hole[card.suit() as usize] += 1;
        }

        for suit in 0..4 {
            // Four to a flush is a draw; five is a made flush, in the made block.
            if suits_all[suit] == 4 && suits_hole[suit] >= 1 {
                mask.set(StatId::FLUSH_DRAW);
            }
            // Backdoor flush draws need two cards to come: a flop statistic.
            if self.board.len() == 3 && suits_all[suit] == 3 {
                if suits_hole[suit] == 2 {
                    mask.set(StatId::BACKDOOR_FLUSH_DRAW_2);
                } else if suits_hole[suit] == 1 && self.options.one_card_backdoor_flushdraw {
                    let contributor = if a.suit() as usize == suit { a } else { b };
                    mask.set(if contributor == combo.high() {
                        StatId::BACKDOOR_FLUSH_DRAW_1_HIGH
                    } else {
                        StatId::BACKDOOR_FLUSH_DRAW_1_LOW
                    });
                }
            }
        }

        self.add_straight_draws(mask, combo);

        let top_board = self.ranks[0];
        let no_pair = made == StatId::ACE_HIGH || made == StatId::NO_MADE_HAND;
        if no_pair && a.rank() > top_board && b.rank() > top_board {
            mask.set(StatId::OVERCARDS);
        }
    }

    /// Straight draws, split by how many hole cards the draw needs.
    ///
    /// Flopzilla reports `oesd (2 card)` and `oesd (1 card)` separately - and the
    /// same for gutshots - because a draw leaning on a single hole card is a
    /// different hand to play. A draw is one-card when a single hole card already
    /// produces the whole draw; if dropping either card costs outs, both are load
    /// bearing and the draw is a two-card one. A double gutshot therefore counts
    /// as two-card even though each hole card contributes a gutshot of its own.
    ///
    /// A draw the board holds on its own belongs to nobody and is not reported.
    fn add_straight_draws(&self, mask: &mut StatMask, combo: Combo) {
        let (a, b) = combo.cards();
        let bit_a = 1u16 << a.rank();
        let bit_b = 1u16 << b.rank();
        let hand_outs = straight_outs(self.rank_mask | bit_a | bit_b);
        if hand_outs & !self.board_outs == 0 {
            return;
        }

        let with_a = straight_outs(self.rank_mask | bit_a);
        let with_b = straight_outs(self.rank_mask | bit_b);
        let one_card = with_a == hand_outs || with_b == hand_outs;
        let open_ended = hand_outs.count_ones() >= 2;

        mask.set(match (open_ended, one_card) {
            (true, false) => StatId::OESD_TWO_CARD,
            (true, true) => StatId::OESD_ONE_CARD,
            (false, false) => StatId::GUTSHOT_TWO_CARD,
            (false, true) => StatId::GUTSHOT_ONE_CARD,
        });
    }
}

/// Classifies one combo against one board.
///
/// Convenience wrapper: when classifying more than a handful of hands against the
/// same board, build a [`BoardContext`] once instead.
pub fn classify(combo: Combo, board: &Board, options: ClassifyOptions) -> StatMask {
    BoardContext::new(board, options).classify(combo)
}

fn no_pair_rung(combo: Combo) -> StatId {
    if combo.high().rank() == RANK_ACE {
        StatId::ACE_HIGH
    } else {
        StatId::NO_MADE_HAND
    }
}

fn add_combinations(mask: &mut StatMask) {
    let has_pair = mask.intersects(PAIR_STATS);
    let fd = mask.has(StatId::FLUSH_DRAW);
    let oesd = mask.intersects(OESD_STATS);
    let gutshot = mask.intersects(GUTSHOT_STATS);
    let overcards = mask.has(StatId::OVERCARDS);

    if fd && has_pair {
        mask.set(StatId::FLUSH_DRAW_PLUS_PAIR);
    }
    if fd && oesd {
        mask.set(StatId::FLUSH_DRAW_PLUS_OESD);
    }
    if fd && gutshot {
        mask.set(StatId::FLUSH_DRAW_PLUS_GUTSHOT);
    }
    if fd && overcards {
        mask.set(StatId::FLUSH_DRAW_PLUS_OVERCARDS);
    }
    if oesd && has_pair {
        mask.set(StatId::OESD_PLUS_PAIR);
    }
    if gutshot && has_pair {
        mask.set(StatId::GUTSHOT_PLUS_PAIR);
    }
    if gutshot && overcards {
        mask.set(StatId::GUTSHOT_PLUS_OVERCARDS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats_for(hand: &str, board: &str) -> StatMask {
        classify(
            Combo::parse(hand).unwrap(),
            &Board::parse(board).unwrap(),
            ClassifyOptions::default(),
        )
    }

    fn made_for(hand: &str, board: &str) -> &'static str {
        let mask = stats_for(hand, board);
        mask.iter()
            .find(|s| s.def().block == StatBlock::Made)
            .expect("every live combo has a made rung")
            .def()
            .key
    }

    fn draw_keys(hand: &str, board: &str) -> Vec<&'static str> {
        stats_for(hand, board)
            .iter()
            .filter(|s| s.def().block == StatBlock::Draw)
            .map(|s| s.def().key)
            .collect()
    }

    #[test]
    fn the_registry_is_consistent() {
        for (index, def) in DEFS.iter().enumerate() {
            assert_eq!(def.id.index() as usize, index, "{} is misnumbered", def.key);
            assert_eq!(stat_by_key(def.key), Some(def.id));
        }
        assert!(DEFS.len() <= 128, "StatMask holds 128 bits");

        // The ladder holds every statistic once and nothing else. The registry
        // may grow at the end - an index is a place in a saved link and may
        // never move - so this is what keeps the two from drifting apart.
        assert_eq!(ORDER.len(), DEFS.len());
        let mut listed: Vec<u8> = ORDER.iter().map(|stat| stat.index()).collect();
        listed.sort_unstable();
        assert_eq!(listed, (0..DEFS.len() as u8).collect::<Vec<_>>());

        // And the blocks are whole in the ladder rather than in the registry,
        // because the ladder is what a reader sees.
        let mut seen: Vec<StatBlock> = Vec::new();
        for def in ORDER.iter().map(|stat| stat.def()) {
            if seen.last() != Some(&def.block) {
                assert!(
                    !seen.contains(&def.block),
                    "block {:?} is split up",
                    def.block
                );
                seen.push(def.block);
            }
        }
        assert_eq!(seen, StatBlock::ALL.to_vec());
    }

    #[test]
    fn derived_masks_match_the_registry() {
        let pairs: StatMask = [
            StatId::OVERPAIR,
            StatId::TOP_PAIR,
            StatId::PP_BELOW_TOP_CARD,
            StatId::SECOND_PAIR,
            StatId::PP_BELOW_SECOND_CARD,
            StatId::BOTTOM_PAIR,
            StatId::PP_BELOW_BOARD,
        ]
        .into_iter()
        .collect();
        assert_eq!(PAIR_STATS, pairs);
        assert_eq!(
            OESD_STATS,
            [StatId::OESD_TWO_CARD, StatId::OESD_ONE_CARD]
                .into_iter()
                .collect::<StatMask>()
        );
        assert_eq!(
            GUTSHOT_STATS,
            [StatId::GUTSHOT_TWO_CARD, StatId::GUTSHOT_ONE_CARD]
                .into_iter()
                .collect::<StatMask>()
        );
    }

    #[test]
    fn the_manual_example_double_counts() {
        // AhKh on KcQhJh is the example from Flopzilla's own manual.
        let mask = stats_for("AhKh", "Kc Qh Jh");
        assert!(mask.has(StatId::TOP_PAIR));
        assert!(mask.has(StatId::FLUSH_DRAW));
        assert!(mask.intersects(GUTSHOT_STATS));
        assert!(mask.has(StatId::FLUSH_DRAW_PLUS_PAIR));
        assert!(mask.has(StatId::FLUSH_DRAW_PLUS_GUTSHOT));
        assert!(mask.has(StatId::GUTSHOT_PLUS_PAIR));
    }

    #[test]
    fn made_hands_land_on_the_right_rung() {
        assert_eq!(made_for("AsKs", "Qs Js Ts"), "straight-flush");
        assert_eq!(made_for("7c7d", "7h 7s 2c"), "quads");
        assert_eq!(made_for("7c7d", "7h 2s 2c"), "full-house");
        assert_eq!(made_for("AcTc", "8c 5c 2c"), "flush");
        assert_eq!(made_for("9c8d", "7h 6s 5c"), "straight");
        assert_eq!(made_for("7c7d", "7h 9s 2c"), "set");
        assert_eq!(made_for("7c9d", "7h 7s 2c"), "trips");
        assert_eq!(made_for("KcQd", "Kh Qs 2c"), "two-pair");
        assert_eq!(made_for("Ac5d", "Kh 9s 2c"), "ace-high");
        assert_eq!(made_for("Jc5d", "Kh 9s 2c"), "no-made-hand");
    }

    #[test]
    fn the_pair_ladder_interleaves_pocket_pairs() {
        // Board K 9 4: the rungs alternate between board pairs and pocket pairs.
        assert_eq!(made_for("AcAd", "Kh 9s 4c"), "overpair");
        assert_eq!(made_for("AcKd", "Kh 9s 4c"), "top-pair");
        assert_eq!(made_for("TcTd", "Kh 9s 4c"), "pp-below-top-card");
        assert_eq!(made_for("Ac9d", "Kh 9s 4c"), "second-pair");
        assert_eq!(made_for("7c7d", "Kh 9s 4c"), "pp-below-2nd-card");
        assert_eq!(made_for("Ac4d", "Kh 9s 4c"), "bottom-pair");
        assert_eq!(made_for("3c3d", "Kh 9s 4c"), "pp-below-board");
    }

    #[test]
    fn a_pair_is_named_for_the_card_it_hit() {
        // Three ranks: top, second, bottom. There is no third pair to have,
        // because the third card is the lowest one and the lowest one is the
        // bottom pair.
        for (hand, rung) in [
            ("AcKd", "top-pair"),
            ("Ac9d", "second-pair"),
            ("Ac4d", "bottom-pair"),
        ] {
            assert_eq!(made_for(hand, "Kh 9s 4c"), rung, "{hand} on a flop");
        }

        // Four ranks: the third card is no longer the lowest, so it gets its
        // own name and the bottom one keeps its.
        for (hand, rung) in [
            ("AcKd", "top-pair"),
            ("Ac9d", "second-pair"),
            ("Ac6h", "third-pair"),
            ("Ac4d", "bottom-pair"),
        ] {
            assert_eq!(made_for(hand, "Kh 9s 6d 4c"), rung, "{hand} on a turn");
        }

        // Five ranks, and the same again with a fourth.
        for (hand, rung) in [
            ("AcKd", "top-pair"),
            ("Ac9d", "second-pair"),
            ("Ac6h", "third-pair"),
            ("Ac4d", "fourth-pair"),
            ("Ac2d", "bottom-pair"),
        ] {
            assert_eq!(made_for(hand, "Kh 9s 6d 4c 2h"), rung, "{hand} on a river");
        }

        // A paired board has fewer ranks than cards, and the names follow the
        // ranks rather than the cards: K K 9 4 is three ranks, so the nine is
        // the second pair and the four is the bottom one.
        for (hand, rung) in [("Ac9h", "second-pair"), ("Ac4d", "bottom-pair")] {
            assert_eq!(
                made_for(hand, "Kh Ks 9d 4c"),
                rung,
                "{hand} on a paired turn"
            );
        }
    }

    #[test]
    fn no_two_rungs_of_the_ladder_mean_one_thing() {
        // The worry this answers: on a board of four ranks the fourth card is
        // also the lowest, and a ladder that offered both "fourth pair" and
        // "bottom pair" would be asking the reader which of two names for one
        // hand to believe. Every board, every card on it, one name each.
        let boards = [
            "Kh 9s 4c",
            "Kh 9s 6d 4c",
            "Kh 9s 6d 4c 2h",
            "Kh Ks 9d 4c",
            "Kh Ks Qd Qc 4h",
            "7h 7s 7c 4d 2h",
        ];
        let pairs = [
            StatId::TOP_PAIR,
            StatId::SECOND_PAIR,
            StatId::THIRD_PAIR,
            StatId::FOURTH_PAIR,
            StatId::BOTTOM_PAIR,
        ];
        for text in boards {
            let board = Board::parse(text).unwrap();
            let mut named: Vec<(&str, Vec<u8>)> = Vec::new();
            for rung in pairs {
                // Which board ranks end up on this rung, whoever holds them.
                let mut ranks: Vec<u8> = Vec::new();
                for combo in Combo::all() {
                    if combo.mask().intersects(board.mask()) {
                        continue;
                    }
                    if !classify(combo, &board, ClassifyOptions::default()).has(rung) {
                        continue;
                    }
                    let (a, b) = combo.cards();
                    for card in [a, b] {
                        if board.mask().iter().any(|on| on.rank() == card.rank())
                            && !ranks.contains(&card.rank())
                        {
                            ranks.push(card.rank());
                        }
                    }
                }
                ranks.sort_unstable();
                if !ranks.is_empty() {
                    named.push((rung.def().key, ranks));
                }
            }
            // No rank on two rungs, and no rung standing for two ranks.
            for (one, (key, ranks)) in named.iter().enumerate() {
                assert_eq!(ranks.len(), 1, "{text}: {key} covers {ranks:?}");
                for (other_key, other) in named.iter().skip(one + 1) {
                    assert_ne!(ranks, other, "{text}: {key} and {other_key} are one thing");
                }
            }
            // And every rank a hand can pair is named by exactly one of them.
            // A rank already paired on the board is not one of those: hitting
            // it makes trips, which is a rung of its own further up.
            let mut counts = [0u8; 13];
            for card in board.mask().iter() {
                counts[card.rank() as usize] += 1;
            }
            // Counted only where no rank is tripled: with three of a kind on
            // the board, pairing one of the other cards is a full house rather
            // than a pair, so there is no rung to land on and nothing to
            // count. That says nothing either way about two rungs meaning one
            // thing, which is checked above for every board.
            if counts.iter().all(|seen| *seen < 3) {
                let pairable = counts.iter().filter(|seen| **seen == 1).count();
                assert_eq!(
                    named.len(),
                    pairable,
                    "{text}: {named:?} against {counts:?}"
                );
            }
        }
    }

    #[test]
    fn board_only_hands_do_not_claim_the_board() {
        // The pair is on the board; the hand contributes nothing.
        assert_eq!(made_for("Ac5d", "Kh Ks 2c"), "ace-high");
        assert_eq!(made_for("Jc5d", "Kh Ks 2c"), "no-made-hand");
        // A pocket pair below a paired board is still a pair, not two pair.
        assert_eq!(made_for("9c9d", "Kh Ks 2c"), "pp-below-top-card");
        // Trips on the board belong to nobody.
        assert_eq!(made_for("Ac5d", "7h 7s 7c"), "ace-high");
        // On the river, a hand that cannot beat the board plays the board.
        assert_eq!(made_for("2c3d", "9h 8s 7c 6d 5h"), "no-made-hand");
        assert_eq!(made_for("Ac3d", "9h 8s 7c 6d 5h"), "ace-high");
        // But a hand that improves on it keeps the straight.
        assert_eq!(made_for("Tc3d", "9h 8s 7c 6d 5h"), "straight");
    }

    #[test]
    fn straight_draws_are_split_by_how_many_hole_cards_they_need() {
        // 98 on 7 6 2 needs both hole cards for the open ender.
        assert!(draw_keys("9c8d", "7h 6s 2c").contains(&"oesd-2"));
        // A single nine turns the board's 8 7 6 into an open ender.
        assert!(draw_keys("9c2d", "8h 7s 6c").contains(&"oesd-1"));
        // 98 on 7 5 2 needs both cards for the gutshot.
        assert!(draw_keys("9c8d", "7h 5s 2c").contains(&"gutshot-2"));
        // A lone jack turns the board's T 9 7 into a one-card gutshot.
        assert!(draw_keys("Jc2d", "Th 9s 7c").contains(&"gutshot-1"));
        // A double gutshot has the same eight outs as an open ender.
        assert!(draw_keys("Jc7d", "9h 8s 5c").contains(&"oesd-2"));
    }

    #[test]
    fn other_draws_are_recognised() {
        assert!(draw_keys("AhQh", "Kh 7h 2c").contains(&"flushdraw"));
        assert!(draw_keys("AcKd", "9h 5s 2c").contains(&"overcards"));
        assert!(draw_keys("AhQh", "Kh 7c 2d").contains(&"bdfd-2"));
    }

    #[test]
    fn a_made_straight_is_not_a_draw() {
        let mask = stats_for("9c8d", "7h 6s 5c");
        assert!(mask.has(StatId::STRAIGHT));
        assert!(!mask.intersects(OESD_STATS));
        assert!(!mask.intersects(GUTSHOT_STATS));
    }

    #[test]
    fn a_board_draw_is_not_a_personal_draw() {
        // The board is open ended by itself; a hand that adds nothing has no draw.
        let mask = stats_for("Ac2d", "9h 8s 7c 6d");
        assert!(!mask.intersects(OESD_STATS));
        assert!(!mask.intersects(GUTSHOT_STATS));
    }

    #[test]
    fn one_card_backdoor_draws_are_opt_in() {
        let hand = Combo::parse("AhQc").unwrap();
        let board = Board::parse("Kh 7h 2d").unwrap();
        let off = classify(hand, &board, ClassifyOptions::default());
        assert!(!off.has(StatId::BACKDOOR_FLUSH_DRAW_1_HIGH));
        let on = classify(
            hand,
            &board,
            ClassifyOptions {
                one_card_backdoor_flushdraw: true,
            },
        );
        assert!(on.has(StatId::BACKDOOR_FLUSH_DRAW_1_HIGH));

        let low = classify(
            Combo::parse("AcQh").unwrap(),
            &board,
            ClassifyOptions {
                one_card_backdoor_flushdraw: true,
            },
        );
        assert!(low.has(StatId::BACKDOOR_FLUSH_DRAW_1_LOW));
    }

    #[test]
    fn draws_disappear_on_the_river() {
        let mask = stats_for("AhQh", "Kh 7h 2c 3d 4s");
        assert!(!mask.has(StatId::FLUSH_DRAW));
    }

    #[test]
    fn every_live_combo_gets_exactly_one_made_rung() {
        let board = Board::parse("Kc Qh Jh").unwrap();
        let stats = ComboStats::build(&board, CardSet::EMPTY, ClassifyOptions::default());
        let made_mask: StatMask = StatId::all()
            .filter(|s| s.def().block == StatBlock::Made)
            .collect();
        let mut live = 0;
        for combo in Combo::all() {
            let mask = stats.mask(combo);
            if combo.mask().intersects(board.mask()) {
                assert!(mask.is_empty());
                continue;
            }
            live += 1;
            assert_eq!(
                mask.intersection(made_mask).len(),
                1,
                "{combo} has the wrong number of made rungs: {mask:?}"
            );
        }
        // 52 cards minus three board cards leaves 49, so C(49,2) live combos.
        assert_eq!(live, 49 * 48 / 2);
        assert_eq!(stats.live().len(), live);
    }

    #[test]
    fn a_hand_holds_at_most_one_straight_draw_rung() {
        for board in ["Kc Qh Jh", "9h 8s 5c", "Th 8s 2h", "7h 6s 2c 5d"] {
            let board = Board::parse(board).unwrap();
            let stats = ComboStats::build(&board, CardSet::EMPTY, ClassifyOptions::default());
            for combo in stats.live().iter() {
                let mask = stats.mask(combo);
                let rungs = mask.intersection(OESD_STATS.union(GUTSHOT_STATS)).len();
                assert!(
                    rungs <= 1,
                    "{combo} on {board} has {rungs} straight-draw rungs"
                );
            }
        }
    }

    #[test]
    fn combos_with_finds_the_overlap() {
        let board = Board::parse("Kc Qh Jh").unwrap();
        let stats = ComboStats::build(&board, CardSet::EMPTY, ClassifyOptions::default());
        let both = stats.matching(
            [StatId::TOP_PAIR, StatId::FLUSH_DRAW]
                .into_iter()
                .collect::<StatMask>(),
        );
        assert!(both.contains(Combo::parse("AhKh").unwrap()));
        assert!(!both.contains(Combo::parse("AcKd").unwrap()));
        for combo in both.iter() {
            let mask = stats.mask(combo);
            assert!(mask.has(StatId::TOP_PAIR) && mask.has(StatId::FLUSH_DRAW));
        }
    }
}
