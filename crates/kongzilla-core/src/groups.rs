//! Colouring a range: which hands belong to which group.
//!
//! This is the shape Flopzilla settles on, and it is a better one than a simple
//! pass/delete pair. A hand is painted a colour, and the colours mean whatever
//! the reader decides - continue, bluff, raise, fold. That does three jobs at
//! once: it names the parts of a range, it gives the pie chart something to
//! measure, and pressing the filter for a street narrows the range to the hands
//! that carry a colour.
//!
//! Two layers, and the split matters. A **category** carries a colour, which is
//! what clicking a row sets. A handful of **exceptions** carry their own, which
//! is what editing a hand inside a category sets. A hand's colour is its
//! exception if it has one, and otherwise the colour of the most recently
//! painted category it belongs to.
//!
//! Storing it this way rather than one colour per hand is what lets a marker
//! tell "this category is blue" from "this category has been picked over", and
//! only the second is a gear. It also means taking a hand out and putting it
//! straight back leaves nothing behind, because the exception is removed again
//! the moment it agrees with its categories.

use crate::cards::{Combo, NUM_COMBOS};
use crate::range::Range;
use crate::stats::{stat_count, ComboStats, StatId};

/// How many colours a hand can be painted, not counting "unpainted".
pub const COLOURS: usize = 5;

/// The palette, in the order it is offered.
///
/// Stable keys rather than indices, so a saved session survives the palette
/// changing size. Five: a strategy that needs a sixth colour has stopped being
/// a strategy anyone can read off the matrix at a glance, and the two that used
/// to follow were close enough to the others to be mistaken for them.
pub const PALETTE: [&str; COLOURS] = ["blue", "green", "red", "violet", "amber"];

/// The colour of one hand: `0` for unpainted, `1..=COLOURS` for a palette entry.
pub type Colour = u8;

/// Unpainted.
pub const NONE: Colour = 0;

/// The colour a new session starts painting with.
pub const DEFAULT_COLOUR: Colour = 1;

/// No exception recorded for this hand; its categories decide.
const INHERIT: Colour = u8::MAX;

/// Reads a palette key into a colour, with `"none"` for unpainted.
pub fn colour_from_key(key: &str) -> Option<Colour> {
    if key == "none" {
        return Some(NONE);
    }
    PALETTE
        .iter()
        .position(|candidate| *candidate == key)
        .map(|index| index as Colour + 1)
}

/// The palette key of a colour.
pub fn colour_key(colour: Colour) -> &'static str {
    match colour {
        NONE => "none",
        _ => PALETTE.get(colour as usize - 1).copied().unwrap_or("none"),
    }
}

/// What a statistic's marker shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    /// The range holds nothing with this statistic, so there is nothing to say.
    Empty,
    /// The category carries one colour, and nothing inside it disagrees.
    Uniform(Colour),
    /// Some of its hands have been given their own colour. Drawn as a gear.
    Mixed,
}

impl Mark {
    /// A stable identifier for the marker, for the boundary and for tests.
    pub fn key(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Mixed => "mixed",
            Self::Uniform(colour) => colour_key(colour),
        }
    }
}

/// How a range is coloured: by category, with exceptions.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GroupSet {
    /// The colour each category was painted.
    categories: Vec<Colour>,
    /// When each was painted, so the last word wins where they overlap.
    painted_at: Vec<u32>,
    /// The next sequence number to hand out.
    clock: u32,
    /// Hands given their own colour, or [`INHERIT`] to follow their categories.
    exceptions: Box<[Colour; NUM_COMBOS]>,
}

impl GroupSet {
    /// Nothing painted.
    pub fn new() -> Self {
        Self {
            categories: vec![NONE; stat_count()],
            painted_at: vec![0; stat_count()],
            clock: 1,
            exceptions: Box::new([INHERIT; NUM_COMBOS]),
        }
    }

    /// The default grouping, which is the one Flopzilla opens with.
    ///
    /// Top pair or better, the flushdraws and the open-enders, and the hands
    /// that are two things at once. Not a recommendation - a starting point that
    /// is right often enough to be worth not retyping.
    pub fn default_for(stats: &ComboStats) -> Self {
        let mut groups = Self::new();
        for stat in DEFAULT_GROUP {
            groups.paint_stat(*stat, DEFAULT_COLOUR, stats);
        }
        groups
    }

    /// The colour of one hand.
    ///
    /// Which categories a hand belongs to depends on the board, so the
    /// classification has to come with the question.
    pub fn get(&self, combo: Combo, stats: &ComboStats) -> Colour {
        let exception = self.exceptions[combo.index() as usize];
        if exception != INHERIT {
            return exception;
        }
        self.inherited(combo, stats)
    }

    /// What a hand's categories alone would make it.
    fn inherited(&self, combo: Combo, stats: &ComboStats) -> Colour {
        // The most recently painted category the hand belongs to: the last thing
        // the reader said about it is the thing they meant.
        let mask = stats.mask(combo);
        let mut best = (0u32, NONE);
        for (index, colour) in self.categories.iter().enumerate() {
            if *colour == NONE || self.painted_at[index] <= best.0 {
                continue;
            }
            let Some(stat) = StatId::from_index(index as u8) else {
                continue;
            };
            if mask.has(stat) {
                best = (self.painted_at[index], *colour);
            }
        }
        best.1
    }

    /// Paints a whole category.
    ///
    /// Any exception inside it goes: saying what the category is settles every
    /// hand in it, which is also how a gear is cleared.
    pub fn paint_stat(&mut self, stat: StatId, colour: Colour, stats: &ComboStats) {
        let index = stat.index() as usize;
        self.categories[index] = colour.min(COLOURS as Colour);
        self.painted_at[index] = self.clock;
        self.clock += 1;
        for combo in stats.live().iter() {
            if stats.mask(combo).has(stat) {
                self.exceptions[combo.index() as usize] = INHERIT;
            }
        }
    }

    /// Gives one hand its own colour, or takes the exception away again.
    pub fn set(&mut self, combo: Combo, colour: Colour, stats: &ComboStats) {
        let colour = colour.min(COLOURS as Colour);
        let slot = combo.index() as usize;
        // A hand set to what its categories already say needs no exception, so
        // taking one out and putting it straight back leaves nothing behind -
        // and no gear on a category nobody meaningfully changed.
        self.exceptions[slot] = if colour == self.inherited(combo, stats) {
            INHERIT
        } else {
            colour
        };
    }

    /// Whether anything is painted at all.
    ///
    /// Asked rather than comparing against a fresh set, because a cleared set
    /// is not structurally identical to a new one - it remembers how many times
    /// it has been painted, which is bookkeeping and not something anyone can
    /// see.
    pub fn is_empty(&self) -> bool {
        self.categories.iter().all(|colour| *colour == NONE)
            && self.exceptions.iter().all(|colour| *colour == INHERIT)
    }

    /// Unpaints everything.
    pub fn clear(&mut self) {
        self.categories.iter_mut().for_each(|c| *c = NONE);
        self.painted_at.iter_mut().for_each(|a| *a = 0);
        self.exceptions.fill(INHERIT);
    }

    /// Swaps which categories are painted, leaving the hands to fall where the
    /// categories put them.
    ///
    /// The other half of inverting, and a different question: this one is about
    /// the marks down the side of the statistics panel - everything I marked
    /// goes bare, everything I did not gets the colour. Because categories
    /// overlap, the hands that come out are *not* the complement of the hands
    /// that went in: a top pair with a gutshot sits in one of each and stays
    /// painted through the flip. That is the honest behaviour of a category
    /// flip, and it is why [`Self::invert`] exists beside it - one inverts the
    /// marks, the other inverts the selection.
    ///
    /// It never needs an exception, so it never grows a gear.
    pub fn invert_categories(&mut self, colour: Colour) {
        let colour = colour.min(COLOURS as Colour);
        for index in 0..self.categories.len() {
            self.categories[index] = if self.categories[index] == NONE {
                self.painted_at[index] = self.clock;
                self.clock += 1;
                colour
            } else {
                NONE
            };
        }
        for slot in 0..NUM_COMBOS {
            if self.exceptions[slot] == INHERIT {
                continue;
            }
            self.exceptions[slot] = if self.exceptions[slot] == NONE {
                colour
            } else {
                NONE
            };
        }
    }

    /// Swaps painted for unpainted, hand by hand.
    ///
    /// "Everything I did not pick" is a statement about hands, not about
    /// categories, and the difference is not academic. Flipping the categories
    /// instead reads as the same thing and is not: categories overlap, so a top
    /// pair that also has a gutshot sits in one painted and one unpainted
    /// category and comes out painted on both sides of the flip. The two halves
    /// then add up to more than the range, which is the one thing an inversion
    /// must never do.
    pub fn invert(&mut self, colour: Colour, stats: &ComboStats) {
        let colour = colour.min(COLOURS as Colour);
        // Read every hand before anything moves: the answer is about the state
        // as it stands, not about the state halfway through being replaced.
        let wanted: Vec<(Combo, Colour)> = stats
            .live()
            .iter()
            .map(|combo| {
                let now = self.get(combo, stats);
                (combo, if now == NONE { colour } else { NONE })
            })
            .collect();

        // What survives an inversion is a set of hands, and the layer that
        // holds hands is the exceptions. The categories cannot hold it: a hand
        // in one painted and one unpainted category has to come out on one
        // side, and no assignment of colours to overlapping categories says
        // which.
        self.categories.iter_mut().for_each(|c| *c = NONE);
        self.painted_at.iter_mut().for_each(|a| *a = 0);
        self.exceptions = Box::new([INHERIT; NUM_COMBOS]);
        for (combo, want) in wanted {
            self.set(combo, want, stats);
        }
    }

    /// What a category's marker shows.
    ///
    /// A gear means the hands in this category have stopped agreeing with each
    /// other, which is the thing a single colour cannot say. It takes both
    /// halves to earn one: somebody picked hands out one at a time *and* the
    /// picking actually split the category.
    ///
    /// Both halves matter. Without the first, a category would wear a gear on
    /// an untouched grouping - categories overlap, so a gutshot that is also
    /// top pair carries top pair's colour while a plain gutshot carries none,
    /// and almost nothing agrees with itself. Without the second, painting the
    /// top of a range by equity would put a gear on every category the cut
    /// swallowed whole, when those are the categories that agree most of all.
    pub fn mark(&self, stat: StatId, range: &Range, stats: &ComboStats) -> Mark {
        let mut held = false;
        let mut picked_over = false;
        let mut colour: Option<Colour> = None;
        let mut agreed = true;
        for combo in stats.live().iter() {
            if range.get(combo) <= 0.0 || !stats.mask(combo).has(stat) {
                continue;
            }
            held = true;
            if self.exceptions[combo.index() as usize] != INHERIT {
                picked_over = true;
            }
            let worn = self.get(combo, stats);
            match colour {
                None => colour = Some(worn),
                Some(first) if first != worn => agreed = false,
                Some(_) => {}
            }
            if picked_over && !agreed {
                break;
            }
        }
        if !held {
            return Mark::Empty;
        }
        if picked_over && !agreed {
            return Mark::Mixed;
        }
        // Picked over but still of one mind: the category wears that colour,
        // whichever layer it arrived from.
        match colour {
            Some(worn) if picked_over => Mark::Uniform(worn),
            _ => Mark::Uniform(self.categories[stat.index() as usize]),
        }
    }

    /// How many colours the range actually uses.
    ///
    /// One colour needs no key to tell it apart, which is what the matrix asks
    /// before deciding whether to mark each cell with which one it is.
    pub fn colours_used(&self, range: &Range, stats: &ComboStats) -> usize {
        let mut seen = [false; COLOURS + 1];
        for combo in stats.live().iter() {
            if range.get(combo) > 0.0 {
                seen[self.get(combo, stats).min(COLOURS as Colour) as usize] = true;
            }
        }
        seen.iter().skip(1).filter(|used| **used).count()
    }

    /// The weight of the range in each colour, unpainted first.
    pub fn shares(&self, range: &Range, stats: &ComboStats) -> [f64; COLOURS + 1] {
        let mut weights = [0.0f64; COLOURS + 1];
        for combo in stats.live().iter() {
            let weight = f64::from(range.get(combo));
            if weight <= 0.0 {
                continue;
            }
            weights[self.get(combo, stats).min(COLOURS as Colour) as usize] += weight;
        }
        weights
    }

    /// The painted part of a range, as a range of its own.
    pub fn painted(&self, range: &Range, stats: &ComboStats) -> Range {
        self.painted_at_weights(range, stats, &[1.0; COLOURS + 1])
    }

    /// The painted part of a range, with each colour passing its own share.
    ///
    /// A colour is how the reader says what they do with a group of hands, and
    /// what they do is not always all-or-nothing: raising a third of your
    /// second pairs is an ordinary thing to do, and a filter that can only keep
    /// all of them or none of them cannot say it. So each colour carries a
    /// share, and a hand that passes carries its own weight times the share of
    /// the colour it wears. An unpainted hand passes nothing, whatever the
    /// shares say.
    pub fn painted_at_weights(
        &self,
        range: &Range,
        stats: &ComboStats,
        shares: &[f32; COLOURS + 1],
    ) -> Range {
        let mut kept = Range::empty();
        for combo in stats.live().iter() {
            let weight = range.get(combo);
            if weight <= 0.0 {
                continue;
            }
            let colour = self.get(combo, stats);
            if colour == NONE {
                continue;
            }
            let share = shares
                .get(colour as usize)
                .copied()
                .unwrap_or(1.0)
                .clamp(0.0, 1.0);
            if share > 0.0 {
                kept.set(combo, weight * share);
            }
        }
        kept
    }

    /// The categories and their colours, in the order they were painted.
    pub fn categories(&self) -> Vec<(StatId, Colour)> {
        let mut painted: Vec<(u32, StatId, Colour)> = self
            .categories
            .iter()
            .enumerate()
            .filter(|(_, colour)| **colour != NONE)
            .filter_map(|(index, colour)| {
                StatId::from_index(index as u8).map(|stat| (self.painted_at[index], stat, *colour))
            })
            .collect();
        painted.sort_by_key(|(at, _, _)| *at);
        painted.into_iter().map(|(_, stat, c)| (stat, c)).collect()
    }

    /// The hands given their own colour, for saving.
    pub fn exceptions(&self) -> impl Iterator<Item = (Combo, Colour)> + '_ {
        self.exceptions
            .iter()
            .enumerate()
            .filter(|(_, colour)| **colour != INHERIT)
            .map(|(index, colour)| (Combo::from_index(index as u16), *colour))
    }
}

impl Default for GroupSet {
    fn default() -> Self {
        Self::new()
    }
}

/// The statistics the default grouping paints.
const DEFAULT_GROUP: &[StatId] = &[
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
    StatId::FLUSH_DRAW,
    StatId::OESD_TWO_CARD,
    StatId::OESD_ONE_CARD,
    StatId::FLUSH_DRAW_PLUS_PAIR,
    StatId::FLUSH_DRAW_PLUS_OESD,
    StatId::FLUSH_DRAW_PLUS_GUTSHOT,
    StatId::FLUSH_DRAW_PLUS_OVERCARDS,
    StatId::OESD_PLUS_PAIR,
    StatId::GUTSHOT_PLUS_PAIR,
    StatId::GUTSHOT_PLUS_OVERCARDS,
];

#[cfg(test)]
mod tests {

    /// A category the reader picked over by hand, but picked over whole.
    #[test]
    fn a_category_picked_over_whole_wears_a_colour_not_a_gear() {
        let (range, stats) = setup("KK", "Kh 7d 2c");
        let mut groups = GroupSet::new();
        let blue: Colour = 1;
        groups.paint_stat(StatId::SET, blue, &stats);
        assert_eq!(
            groups.mark(StatId::SET, &range, &stats),
            Mark::Uniform(blue)
        );

        // Pick every one of the three live kings out by hand, all the same
        // colour. Nothing about the category has been split, so nothing about
        // it needs a gear: it is still entirely one colour.
        let green: Colour = 2;
        for combo in stats.live().iter() {
            if range.get(combo) > 0.0 && stats.mask(combo).has(StatId::SET) {
                groups.set(combo, green, &stats);
            }
        }
        assert_eq!(
            groups.mark(StatId::SET, &range, &stats),
            Mark::Uniform(green),
            "picked over, but of one mind"
        );

        // Split it, and the gear is the only thing that can say so.
        let one = stats
            .live()
            .iter()
            .find(|combo| range.get(*combo) > 0.0 && stats.mask(*combo).has(StatId::SET))
            .unwrap();
        groups.set(one, NONE, &stats);
        assert_eq!(groups.mark(StatId::SET, &range, &stats), Mark::Mixed);
    }
    use super::*;
    use crate::board::Board;
    use crate::cards::CardSet;
    use crate::stats::ClassifyOptions;

    fn setup(text: &str, board: &str) -> (Range, ComboStats) {
        let board = Board::parse(board).unwrap();
        let stats = ComboStats::build(&board, CardSet::EMPTY, ClassifyOptions::default());
        (Range::parse(text).unwrap(), stats)
    }

    #[test]
    fn painting_a_category_marks_it_and_nothing_else() {
        let (range, stats) = setup("22+,A2s+,KJs+,AJo+", "Kh 7h 2c");
        let mut groups = GroupSet::new();
        assert_eq!(
            groups.mark(StatId::TOP_PAIR, &range, &stats),
            Mark::Uniform(NONE)
        );

        groups.paint_stat(StatId::TOP_PAIR, 1, &stats);
        assert_eq!(
            groups.mark(StatId::TOP_PAIR, &range, &stats),
            Mark::Uniform(1)
        );
        // And the categories it overlaps are still their own colour, because a
        // category's marker is about the category, not about every last hand.
        assert_eq!(
            groups.mark(StatId::FLUSH_DRAW, &range, &stats),
            Mark::Uniform(NONE)
        );
    }

    #[test]
    fn the_last_category_painted_wins_where_they_overlap() {
        let (range, stats) = setup("22+,A2s+,KJs+,AJo+", "Kh 7h 2c");
        let mut groups = GroupSet::new();
        let both = crate::cards::Combo::parse("AhQh").unwrap();
        assert!(stats.mask(both).has(StatId::FLUSH_DRAW));
        assert!(stats.mask(both).has(StatId::ACE_HIGH));

        groups.paint_stat(StatId::ACE_HIGH, 1, &stats);
        assert_eq!(groups.get(both, &stats), 1);
        // Saying "flushdraws are green" after "ace high is blue" means the hand
        // that is both comes out green: the last thing said is what was meant.
        groups.paint_stat(StatId::FLUSH_DRAW, 2, &stats);
        assert_eq!(groups.get(both, &stats), 2);
        // Neither category is a gear: nothing was picked over by hand.
        assert_eq!(
            groups.mark(StatId::ACE_HIGH, &range, &stats),
            Mark::Uniform(1)
        );
        assert_eq!(
            groups.mark(StatId::FLUSH_DRAW, &range, &stats),
            Mark::Uniform(2)
        );
    }

    #[test]
    fn taking_a_hand_out_and_putting_it_back_leaves_no_gear() {
        let (range, stats) = setup("22+,A2s+,KJs+,AJo+", "Kh 7h 2c");
        let mut groups = GroupSet::new();
        groups.paint_stat(StatId::FLUSH_DRAW, 1, &stats);
        let one = crate::cards::Combo::parse("AhQh").unwrap();

        // Out: the hand disagrees with its category, so the category is a gear.
        groups.set(one, NONE, &stats);
        assert_eq!(groups.get(one, &stats), NONE);
        assert_eq!(groups.mark(StatId::FLUSH_DRAW, &range, &stats), Mark::Mixed);

        // Back: nothing has changed, so there is nothing to warn about. Doing
        // and undoing is not the same as doing.
        groups.set(one, 1, &stats);
        assert_eq!(groups.get(one, &stats), 1);
        assert_eq!(
            groups.mark(StatId::FLUSH_DRAW, &range, &stats),
            Mark::Uniform(1)
        );
    }

    #[test]
    fn repainting_a_category_settles_every_hand_in_it() {
        let (range, stats) = setup("22+,A2s+,KJs+,AJo+", "Kh 7h 2c");
        let mut groups = GroupSet::new();
        groups.paint_stat(StatId::FLUSH_DRAW, 1, &stats);
        groups.set(crate::cards::Combo::parse("AhQh").unwrap(), 3, &stats);
        assert_eq!(groups.mark(StatId::FLUSH_DRAW, &range, &stats), Mark::Mixed);

        groups.paint_stat(StatId::FLUSH_DRAW, 1, &stats);
        assert_eq!(
            groups.mark(StatId::FLUSH_DRAW, &range, &stats),
            Mark::Uniform(1)
        );
    }

    #[test]
    fn a_category_the_range_cannot_make_is_empty_not_unpainted() {
        let (range, stats) = setup("AhAd", "Kh 7h 2c");
        let groups = GroupSet::new();
        assert_eq!(
            groups.mark(StatId::OVERPAIR, &range, &stats),
            Mark::Uniform(NONE)
        );
        assert_eq!(
            groups.mark(StatId::STRAIGHT_FLUSH, &range, &stats),
            Mark::Empty
        );
    }

    #[test]
    fn the_default_grouping_is_top_pair_or_better_plus_the_real_draws() {
        let (range, stats) = setup("22+,A2s+,KJs+,AJo+", "Kh 7h 2c");
        let groups = GroupSet::default_for(&stats);

        assert_eq!(groups.mark(StatId::TWO_PAIR, &range, &stats), Mark::Empty);
        for stat in [
            StatId::TOP_PAIR,
            StatId::OVERPAIR,
            StatId::SET,
            StatId::FLUSH_DRAW,
        ] {
            assert_eq!(groups.mark(stat, &range, &stats).key(), "blue", "{stat:?}");
        }
        // The categories it does not paint read as unpainted, and not one of
        // them is a gear - nobody has picked anything over. A category this
        // range cannot make on this board reads as empty, which is neither.
        for stat in [StatId::MIDDLE_PAIR, StatId::ACE_HIGH] {
            assert_eq!(groups.mark(stat, &range, &stats).key(), "none", "{stat:?}");
        }
        for stat in StatId::all() {
            assert_ne!(groups.mark(stat, &range, &stats), Mark::Mixed, "{stat:?}");
        }
    }

    #[test]
    fn the_shares_add_up_to_the_range() {
        let (range, stats) = setup("22+,A2s+,KJs+,AJo+", "Kh 7h 2c");
        let groups = GroupSet::default_for(&stats);
        let shares = groups.shares(&range, &stats);
        let total: f64 = shares.iter().sum();
        assert!((total - range.combo_count_excluding(stats.board().mask())).abs() < 1e-9);
        assert!(shares[0] > 0.0, "something should be left unpainted");
        assert!(shares[1] > 0.0, "the default paints something");

        let painted = groups.painted(&range, &stats);
        assert!((painted.combo_count() - (total - shares[0])).abs() < 1e-9);
        // One colour in use, so the matrix has nothing to tell apart.
        assert_eq!(groups.colours_used(&range, &stats), 1);
    }

    #[test]
    fn inverting_gives_back_exactly_what_was_not_painted() {
        let (range, stats) = setup("22+,A2s+,KJs+,AJo+", "Kh 7h 2c");
        let mut groups = GroupSet::default_for(&stats);
        let before = groups.shares(&range, &stats);
        assert!(before[0] > 0.0 && before[1] > 0.0, "something of each");

        groups.invert(1, &stats);
        let after = groups.shares(&range, &stats);

        // The point of the button: what is painted now is what was not painted
        // then, hand for hand. The two halves are one range said twice.
        assert!(
            (after[1] - before[0]).abs() < 1e-9,
            "painted after should be unpainted before: {after:?} against {before:?}"
        );
        assert!(
            (after[0] - before[1]).abs() < 1e-9,
            "{after:?} against {before:?}"
        );

        // A category that was painted whole is bare afterwards.
        assert_eq!(
            groups.mark(StatId::TOP_PAIR, &range, &stats),
            Mark::Uniform(NONE)
        );

        // Twice is where it started.
        groups.invert(1, &stats);
        let back = groups.shares(&range, &stats);
        for colour in 0..back.len() {
            assert!(
                (back[colour] - before[colour]).abs() < 1e-9,
                "inverting twice is doing nothing: {back:?} against {before:?}"
            );
        }
    }

    #[test]
    fn inverting_splits_the_categories_that_overlap_a_painted_one() {
        let (range, stats) = setup("22+,A2s+,KJs+,AJo+", "Kh 7h 2c");
        let mut groups = GroupSet::default_for(&stats);
        groups.invert(1, &stats);

        // A gutshot that is also a top pair was painted before and is bare now;
        // a gutshot that is nothing else was bare and is painted now. So the
        // category holds both, and the gear is the only marker that says so.
        // This is the honest cost of inverting hands rather than categories,
        // and the alternative is two halves that add up to more than the range.
        let split = StatId::all()
            .filter(|stat| groups.mark(*stat, &range, &stats) == Mark::Mixed)
            .count();
        assert!(split > 0, "overlapping categories have to come out split");
    }

    #[test]
    fn the_layers_can_be_written_down_and_read_back() {
        let (_, stats) = setup("22+", "Kh 7h 2c");
        let mut groups = GroupSet::new();
        groups.paint_stat(StatId::TOP_PAIR, 1, &stats);
        groups.paint_stat(StatId::FLUSH_DRAW, 2, &stats);
        groups.set(crate::cards::Combo::parse("AhQh").unwrap(), 3, &stats);

        // Categories come back in the order they were painted, which is what
        // decides the winner where two of them overlap.
        assert_eq!(
            groups.categories(),
            vec![(StatId::TOP_PAIR, 1), (StatId::FLUSH_DRAW, 2)]
        );
        assert_eq!(
            groups.exceptions().collect::<Vec<_>>(),
            vec![(crate::cards::Combo::parse("AhQh").unwrap(), 3)]
        );
    }
}
