//! How often each kind of flop comes.
//!
//! Flopzilla's flop breakdown tool, which answers questions the statistics panel
//! cannot: how often the flop is paired, monotone, ace-high, connected - and how
//! much your own two cards change those numbers. Blockers matter more than people
//! expect: ace-high flops are common until you are the one holding an ace.

use crate::board::Board;
use crate::cards::CardSet;
use crate::rng::Rng;

/// One bucket within an axis.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct FlopGroup {
    /// A stable identifier.
    pub key: String,
    /// The name shown in the panel.
    pub label: String,
    /// How many flops fall in the bucket.
    pub flops: u64,
    /// Share of all counted flops, in `0.0..=1.0`.
    pub fraction: f64,
    /// How many of those also satisfy the ticks on *other* axes.
    ///
    /// Its own axis is left out on purpose. Ticking monotone and then reading
    /// the high-card rows as zeroes would answer a question nobody asked; what
    /// the reader wants there is how the high card falls *among* monotone
    /// flops, which is what lets them narrow one axis at a time. So each row
    /// answers "of my other conditions, how much is this" - and what is
    /// actually selected is said by the tick, not by the bar.
    ///
    /// With nothing ticked anywhere this equals [`Self::flops`].
    pub kept: u64,
    /// Share of all counted flops that are in this bucket *and* kept.
    pub kept_fraction: f64,
}

/// One way of cutting up the flops.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct FlopAxis {
    /// A stable identifier.
    pub key: &'static str,
    /// The heading shown above the buckets.
    pub label: &'static str,
    /// The buckets, which partition every counted flop.
    pub groups: Vec<FlopGroup>,
}

/// Every axis, counted over the flops the dead cards leave available.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct FlopBreakdown {
    /// How many flops were counted.
    pub total: u64,
    /// How many of them satisfy every tick, which is what a pass runs over.
    pub kept: u64,
    /// The axes, each a full partition of those flops.
    pub axes: Vec<FlopAxis>,
}

impl FlopBreakdown {
    /// Looks a bucket up by axis and bucket key.
    pub fn group(&self, axis: &str, group: &str) -> Option<&FlopGroup> {
        self.axes
            .iter()
            .find(|candidate| candidate.key == axis)?
            .groups
            .iter()
            .find(|candidate| candidate.key == group)
    }
}

/// Counts the flops by every axis at once.
pub fn breakdown(dead: CardSet) -> FlopBreakdown {
    filtered_breakdown(dead, FlopFilter::EVERYTHING)
}

/// The same counts, and how much of each survives the ticks on the other axes.
///
/// One pass rather than five: every flop is bucketed on all four axes anyway,
/// and once it is, whether it satisfies the ticks on the other three is four
/// comparisons rather than another walk of 22,100 boards.
pub fn filtered_breakdown(dead: CardSet, filter: FlopFilter) -> FlopBreakdown {
    let mut counts: [Vec<u64>; AXIS_KEYS.len()] =
        std::array::from_fn(|at| vec![0u64; GROUP_KEYS[at].len()]);
    let mut kept: [Vec<u64>; AXIS_KEYS.len()] =
        std::array::from_fn(|at| vec![0u64; GROUP_KEYS[at].len()]);
    let mut total = 0u64;
    let mut all_kept = 0u64;

    for flop in Board::all_flops() {
        if flop.mask().intersects(dead) {
            continue;
        }
        total += 1;

        let ranks = flop.ranks_desc();
        let rank_counts = flop.rank_counts();
        let fell_in = [
            match ranks.len() {
                1 => 3,
                // The doubled rank is either the higher or the lower of the two.
                2 if rank_counts[ranks[0] as usize] == 2 => 1,
                2 => 2,
                _ => 0,
            },
            3 - flop.suit_counts().iter().filter(|c| **c > 0).count(),
            match ranks[0] {
                12 => 0,
                11 => 1,
                10 => 2,
                9 => 3,
                8 => 4,
                _ => 5,
            },
            connectedness(&ranks),
        ];

        let bits = filter.bits();
        let asked: [bool; AXIS_KEYS.len()] =
            std::array::from_fn(|at| bits[at] == 0 || bits[at] & (1 << fell_in[at]) != 0);
        if asked.iter().all(|passes| *passes) {
            all_kept += 1;
        }
        for at in 0..AXIS_KEYS.len() {
            counts[at][fell_in[at]] += 1;
            // Every axis but this one, so a row says what it is worth under the
            // reader's other conditions rather than under its own tick.
            if asked
                .iter()
                .enumerate()
                .all(|(other, passes)| other == at || *passes)
            {
                kept[at][fell_in[at]] += 1;
            }
        }
    }

    let share = |count: u64| {
        if total > 0 {
            count as f64 / total as f64
        } else {
            0.0
        }
    };
    let group = |axis: usize, at: usize, key: &str, label: &str| FlopGroup {
        key: key.to_owned(),
        label: label.to_owned(),
        flops: counts[axis][at],
        fraction: share(counts[axis][at]),
        kept: kept[axis][at],
        kept_fraction: share(kept[axis][at]),
    };
    FlopBreakdown {
        total,
        kept: all_kept,
        axes: (0..AXIS_KEYS.len())
            .map(|at| FlopAxis {
                key: AXIS_KEYS[at],
                label: AXIS_LABELS[at],
                groups: (0..GROUP_KEYS[at].len())
                    .map(|index| group(at, index, GROUP_KEYS[at][index], GROUP_LABELS[at][index]))
                    .collect(),
            })
            .collect(),
    }
}

/// The axes, in the order the panel shows them.
///
/// The filter below addresses groups by their position in this list rather than
/// by name, so this is the one place that fixes the order.
pub const AXIS_KEYS: [&str; 4] = ["pairing", "suits", "high-card", "connectedness"];

/// Every group key, by axis, in the order the panel shows them.
///
/// [`breakdown`] builds the same keys as it counts; a test holds the two to
/// each other. They are repeated here because a filter has to name a group
/// without counting 22,100 flops to find out what the groups are called.
pub const GROUP_KEYS: [&[&str]; AXIS_KEYS.len()] = [
    &["unpaired", "paired-top", "paired-bottom", "trips"],
    &["rainbow", "two-tone", "monotone"],
    &["A", "K", "Q", "J", "T", "low"],
    &["three", "two", "none"],
];

/// The heading each axis is shown under.
const AXIS_LABELS: [&str; AXIS_KEYS.len()] = ["Pairing", "Suits", "High card", "Connectedness"];

/// What each group is called, in step with [`GROUP_KEYS`].
const GROUP_LABELS: [&[&str]; AXIS_KEYS.len()] = [
    &[
        "Unpaired",
        "Paired, top card",
        "Paired, bottom card",
        "Trips",
    ],
    &["Rainbow", "Two-tone", "Monotone"],
    &[
        "A high",
        "K high",
        "Q high",
        "J high",
        "T high",
        "9 high or lower",
    ],
    &["Three to a straight", "Two to a straight", "Disconnected"],
];

/// Which flops a pass should look at.
///
/// A pass over all 22,100 flops says what a range does on average. It does not
/// say what it does on the flops you actually care about - and "how does this
/// range do on two-tone, ace-high boards" is a different question with a
/// different answer.
///
/// Within one axis the chosen groups are alternatives: ticking rainbow and
/// two-tone means either. Across axes they are conditions: two-tone *and* ace
/// high. That is the reading the boxes suggest, and the only one that lets a
/// reader narrow rather than widen as they tick. An axis with nothing ticked
/// asks nothing of the flop, so an empty filter is every flop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlopFilter {
    /// One bit per group, one byte per axis.
    axes: [u8; AXIS_KEYS.len()],
}

impl FlopFilter {
    /// A filter that asks nothing, which is every flop.
    pub const EVERYTHING: Self = Self {
        axes: [0; AXIS_KEYS.len()],
    };

    /// The raw group bits, one byte per axis, for hashing and nothing else.
    pub fn bits(&self) -> [u8; AXIS_KEYS.len()] {
        self.axes
    }

    /// Whether this asks nothing of the flops.
    pub fn is_empty(&self) -> bool {
        self.axes.iter().all(|bits| *bits == 0)
    }

    /// Whether one group is ticked.
    pub fn contains(&self, axis: &str, group: &str) -> bool {
        let Some((axis, group)) = indices(axis, group) else {
            return false;
        };
        self.axes[axis] & (1 << group) != 0
    }

    /// Ticks or unticks one group. `false` when there is no such group.
    pub fn toggle(&mut self, axis: &str, group: &str) -> bool {
        let Some((axis, group)) = indices(axis, group) else {
            return false;
        };
        self.axes[axis] ^= 1 << group;
        true
    }

    /// Unticks everything.
    pub fn clear(&mut self) {
        self.axes = [0; AXIS_KEYS.len()];
    }

    /// Whether a flop is one of the ones asked for.
    pub fn matches(&self, flop: &Board) -> bool {
        for (at, bits) in self.axes.iter().enumerate() {
            if *bits == 0 {
                continue;
            }
            let Some(fell_in) = bucket(flop, AXIS_KEYS[at]) else {
                continue;
            };
            if bits & (1 << fell_in) == 0 {
                return false;
            }
        }
        true
    }

    /// How many flops the filter leaves, once the dead cards are out.
    pub fn count(&self, dead: CardSet) -> u64 {
        Board::all_flops()
            .filter(|flop| !flop.mask().intersects(dead) && self.matches(flop))
            .count() as u64
    }

    /// The ticked groups, as `(axis, group)` key pairs in panel order.
    pub fn selected(&self) -> Vec<(&'static str, &'static str)> {
        let mut out = Vec::new();
        for (at, bits) in self.axes.iter().enumerate() {
            for (index, group) in GROUP_KEYS[at].iter().enumerate() {
                if bits & (1 << index) != 0 {
                    out.push((AXIS_KEYS[at], *group));
                }
            }
        }
        out
    }

    /// The filter as one character per axis, trailing nothing trimmed.
    ///
    /// Six groups fit in six bits, so an axis is one digit of the link's
    /// alphabet and the whole filter costs four characters at the very most.
    pub fn to_code(&self, digit: impl Fn(usize) -> char) -> String {
        let last = self.axes.iter().rposition(|bits| *bits != 0);
        match last {
            None => String::new(),
            Some(last) => self.axes[..=last]
                .iter()
                .map(|bits| digit(*bits as usize))
                .collect(),
        }
    }

    /// And back. Characters outside the alphabet are read as nothing ticked.
    pub fn from_code(code: &str, value: impl Fn(char) -> Option<usize>) -> Self {
        let mut filter = Self::EVERYTHING;
        for (at, c) in code.chars().take(AXIS_KEYS.len()).enumerate() {
            let bits = value(c).unwrap_or(0);
            // A bit for a group the axis does not have would match no flop and
            // could never be unticked, so it is dropped on the way in.
            filter.axes[at] = (bits as u8) & mask_of(GROUP_KEYS[at].len());
        }
        filter
    }
}

/// The low `width` bits set.
fn mask_of(width: usize) -> u8 {
    ((1u16 << width) - 1) as u8
}

/// Where an axis and a group sit in the fixed order.
fn indices(axis: &str, group: &str) -> Option<(usize, usize)> {
    let at = AXIS_KEYS.iter().position(|key| *key == axis)?;
    let group = GROUP_KEYS[at].iter().position(|key| *key == group)?;
    Some((at, group))
}

/// Which bucket a flop falls into, by axis key.
///
/// The counting pass and the sampler have to agree about what "two-tone" means,
/// so they read it from here rather than each working it out.
fn bucket(flop: &Board, axis: &str) -> Option<usize> {
    let ranks = flop.ranks_desc();
    match axis {
        "pairing" => Some(match ranks.len() {
            1 => 3,
            2 => {
                if flop.rank_counts()[ranks[0] as usize] == 2 {
                    1
                } else {
                    2
                }
            }
            _ => 0,
        }),
        "suits" => {
            let distinct = flop
                .suit_counts()
                .iter()
                .filter(|count| **count > 0)
                .count();
            Some(3 - distinct)
        }
        "high-card" => Some(match ranks[0] {
            12 => 0,
            11 => 1,
            10 => 2,
            9 => 3,
            8 => 4,
            _ => 5,
        }),
        "connectedness" => Some(connectedness(&ranks)),
        _ => None,
    }
}

/// A flop drawn at random from the ones a filter leaves.
///
/// The same reservoir walk [`sample`] does, over a different question: not
/// "one two-tone flop" but "one of the flops I am looking at". An empty filter
/// leaves every flop, so this is also the plain random deal.
pub fn sample_matching(filter: FlopFilter, dead: CardSet, rng: &mut Rng) -> Option<Board> {
    let mut seen = 0u32;
    let mut chosen = None;
    for flop in Board::all_flops() {
        if flop.mask().intersects(dead) || !filter.matches(&flop) {
            continue;
        }
        seen += 1;
        if rng.below(seen) == 0 {
            chosen = Some(flop);
        }
    }
    chosen
}

/// A flop drawn at random from one bucket of one axis.
///
/// Reading "two-tone flops are 55% of them" and then having to think one up is
/// the slow half of the work, so the panel deals one instead. Reservoir
/// sampling, because the bucket can be most of the 22,100 and none of it needs
/// to be held at once.
pub fn sample(axis: &str, group: &str, dead: CardSet, rng: &mut Rng) -> Option<Board> {
    let wanted = breakdown(CardSet::EMPTY)
        .axes
        .iter()
        .find(|candidate| candidate.key == axis)?
        .groups
        .iter()
        .position(|candidate| candidate.key == group)?;

    let mut seen = 0u32;
    let mut chosen = None;
    for flop in Board::all_flops() {
        if flop.mask().intersects(dead) || bucket(&flop, axis) != Some(wanted) {
            continue;
        }
        seen += 1;
        if rng.below(seen) == 0 {
            chosen = Some(flop);
        }
    }
    chosen
}

/// `0` when all three ranks sit inside a five-rank window, `1` when only two do,
/// `2` otherwise.
///
/// The ace plays low as well as high, so `A23` is three to a straight and `A92`
/// is two to one, through the wheel.
fn connectedness(ranks: &[u8]) -> usize {
    fn spans(sorted: &[i16]) -> usize {
        match sorted {
            [high, .., low] if high - low <= 4 => 0,
            [high, middle, low] if high - middle <= 4 || middle - low <= 4 => 1,
            [high, low] if high - low <= 4 => 1,
            _ => 2,
        }
    }

    let high: Vec<i16> = ranks.iter().map(|rank| i16::from(*rank)).collect();
    let mut best = spans(&high);
    if ranks.contains(&12) {
        // An ace below the deuce, so the wheel is contiguous.
        let mut low: Vec<i16> = ranks
            .iter()
            .map(|rank| if *rank == 12 { -1 } else { i16::from(*rank) })
            .collect();
        low.sort_unstable_by(|a, b| b.cmp(a));
        best = best.min(spans(&low));
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_group_is_named_counted_and_addressable() {
        // The filter addresses groups by position; the panel shows what the
        // counter built; the labels come from a third table. They are the same
        // groups or the panel ticks the wrong rows.
        let counted = breakdown(CardSet::EMPTY);
        let axes: Vec<&str> = counted.axes.iter().map(|axis| axis.key).collect();
        assert_eq!(axes, AXIS_KEYS.to_vec());
        for (at, axis) in counted.axes.iter().enumerate() {
            let keys: Vec<&str> = axis.groups.iter().map(|g| g.key.as_str()).collect();
            assert_eq!(keys, GROUP_KEYS[at].to_vec(), "axis {}", axis.key);
            assert_eq!(
                GROUP_LABELS[at].len(),
                GROUP_KEYS[at].len(),
                "axis {}",
                axis.key
            );
            assert!(
                axis.groups.iter().all(|group| !group.label.is_empty()),
                "axis {} has an unnamed group",
                axis.key
            );
        }
        // The groups of an axis partition the flops, which is what lets the
        // percentages be read down a column.
        for axis in &counted.axes {
            let summed: u64 = axis.groups.iter().map(|group| group.flops).sum();
            assert_eq!(summed, counted.total, "axis {} loses flops", axis.key);
        }
    }

    #[test]
    fn a_row_says_what_it_is_worth_under_the_other_axes() {
        let plain = breakdown(CardSet::EMPTY);

        // Nothing ticked: a row keeps everything it counts, so the panel reads
        // exactly as it did before there was a filter at all.
        for axis in &plain.axes {
            for group in &axis.groups {
                assert_eq!(group.kept, group.flops, "{}/{}", axis.key, group.key);
            }
        }
        assert_eq!(plain.kept, plain.total);

        // Tick monotone. Its own axis is left out of its own rows, so the suit
        // rows are untouched - the tick says what is selected there, not the
        // bar.
        let mut filter = FlopFilter::EVERYTHING;
        filter.toggle("suits", "monotone");
        let counted = filtered_breakdown(CardSet::EMPTY, filter);
        let suits = counted.axes.iter().find(|a| a.key == "suits").unwrap();
        for group in &suits.groups {
            assert_eq!(group.kept, group.flops, "suits/{}", group.key);
        }

        // Every other axis is now counted over monotone flops only, so its rows
        // add up to the monotone count rather than to all of them.
        let monotone = plain.group("suits", "monotone").unwrap().flops;
        assert_eq!(counted.kept, monotone);
        for axis in counted.axes.iter().filter(|a| a.key != "suits") {
            let summed: u64 = axis.groups.iter().map(|group| group.kept).sum();
            assert_eq!(summed, monotone, "axis {}", axis.key);
            // And each row still counts its own bucket in full beside it, which
            // is the part of the bar that says what the ticking cost.
            for group in &axis.groups {
                assert!(group.kept <= group.flops, "{}/{}", axis.key, group.key);
            }
        }

        // Trips cannot be monotone - three of a rank is three of three suits -
        // so that row keeps nothing, which is how the panel knows to stop the
        // reader ticking it.
        let trips = counted.group("pairing", "trips").unwrap();
        assert_eq!(trips.kept, 0);
        assert!(trips.flops > 0, "trips exist, they are just not monotone");
    }

    #[test]
    fn a_filter_narrows_within_an_axis_and_across_them() {
        let counted = breakdown(CardSet::EMPTY);
        let all = counted.total;

        // Nothing ticked asks nothing: every flop is one of the ones wanted.
        assert!(FlopFilter::EVERYTHING.is_empty());
        assert_eq!(FlopFilter::EVERYTHING.count(CardSet::EMPTY), all);

        // One group is that group, exactly as the panel counted it.
        let mut filter = FlopFilter::EVERYTHING;
        assert!(filter.toggle("suits", "two-tone"));
        let two_tone = counted.group("suits", "two-tone").unwrap().flops;
        assert_eq!(filter.count(CardSet::EMPTY), two_tone);

        // A second group on the same axis is an alternative, so the count is
        // the two added together.
        assert!(filter.toggle("suits", "monotone"));
        let monotone = counted.group("suits", "monotone").unwrap().flops;
        assert_eq!(filter.count(CardSet::EMPTY), two_tone + monotone);

        // A group on another axis is a condition, so the count can only fall,
        // and it falls below either side on its own.
        assert!(filter.toggle("high-card", "A"));
        let with_ace = filter.count(CardSet::EMPTY);
        let aces = counted.group("high-card", "A").unwrap().flops;
        assert!(with_ace < two_tone + monotone);
        assert!(with_ace < aces);

        // Every flop it counts really is one of each.
        for flop in Board::all_flops().filter(|flop| filter.matches(flop)) {
            assert_eq!(bucket(&flop, "high-card"), Some(0), "{flop}");
            assert!(matches!(bucket(&flop, "suits"), Some(1 | 2)), "{flop}");
        }

        // Ticking twice puts it back.
        assert!(filter.toggle("high-card", "A"));
        assert_eq!(filter.count(CardSet::EMPTY), two_tone + monotone);
        filter.clear();
        assert!(filter.is_empty());
    }

    #[test]
    fn the_axes_of_a_filter_partition_their_flops() {
        // Ticking every group of one axis is the same as ticking none of it:
        // the groups are a partition, so either way every flop qualifies.
        let mut every = FlopFilter::EVERYTHING;
        for group in GROUP_KEYS[2] {
            assert!(every.toggle("high-card", group));
        }
        assert!(!every.is_empty(), "something is ticked");
        assert_eq!(
            every.count(CardSet::EMPTY),
            breakdown(CardSet::EMPTY).total,
            "a full axis excludes nothing"
        );
    }

    #[test]
    fn a_filter_travels_as_a_few_characters() {
        let digits = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-_";
        let to_digit = |value: usize| char::from(digits[value.min(63)]);
        let from_digit = |c: char| digits.iter().position(|d| char::from(*d) == c);

        let mut filter = FlopFilter::EVERYTHING;
        filter.toggle("suits", "two-tone");
        filter.toggle("high-card", "A");
        filter.toggle("high-card", "K");
        let code = filter.to_code(to_digit);
        assert!(code.len() <= 4, "{code} is longer than one digit per axis");
        assert_eq!(FlopFilter::from_code(&code, from_digit), filter);

        // Nothing ticked costs nothing.
        assert_eq!(FlopFilter::EVERYTHING.to_code(to_digit), "");
        assert!(FlopFilter::from_code("", from_digit).is_empty());

        // A bit for a group that does not exist cannot be read in, because
        // nothing in the panel could ever untick it again.
        let strays = FlopFilter::from_code("__", from_digit);
        assert_eq!(
            strays.selected().len(),
            GROUP_KEYS[0].len() + GROUP_KEYS[1].len()
        );
    }

    #[test]
    fn a_sampled_flop_belongs_to_the_bucket_it_was_asked_for() {
        let mut rng = Rng::new(7);
        let counts = breakdown(CardSet::EMPTY);
        for axis in &counts.axes {
            for group in &axis.groups {
                let flop = flops_sample(axis.key, &group.key, &mut rng);
                assert_eq!(flop.len(), 3, "{}/{} dealt no flop", axis.key, group.key);
                let wanted = axis.groups.iter().position(|g| g.key == group.key).unwrap();
                assert_eq!(
                    bucket(&flop, axis.key),
                    Some(wanted),
                    "{}/{} dealt {flop}",
                    axis.key,
                    group.key
                );
            }
        }
    }

    #[test]
    fn a_sampler_spreads_over_the_bucket_and_avoids_the_dead_cards() {
        let mut rng = Rng::new(11);
        let dead = CardSet::parse("As Ks Qs").unwrap();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..40 {
            let flop = sample("suits", "monotone", dead, &mut rng).unwrap();
            assert!(!flop.mask().intersects(dead), "{flop} used a dead card");
            assert_eq!(bucket(&flop, "suits"), Some(2));
            seen.insert(flop.to_string());
        }
        assert!(
            seen.len() > 20,
            "only {} distinct flops in 40 deals",
            seen.len()
        );
    }

    #[test]
    fn an_unknown_bucket_deals_nothing() {
        let mut rng = Rng::new(3);
        assert!(sample("suits", "polychrome", CardSet::EMPTY, &mut rng).is_none());
        assert!(sample("texture", "wet", CardSet::EMPTY, &mut rng).is_none());
    }

    fn flops_sample(axis: &str, group: &str, rng: &mut Rng) -> Board {
        sample(axis, group, CardSet::EMPTY, rng).expect("every bucket has flops")
    }

    fn full() -> FlopBreakdown {
        breakdown(CardSet::EMPTY)
    }

    #[test]
    fn every_axis_partitions_every_flop() {
        let result = full();
        assert_eq!(result.total, 22_100);
        for axis in &result.axes {
            let counted: u64 = axis.groups.iter().map(|group| group.flops).sum();
            assert_eq!(
                counted, result.total,
                "axis {} does not partition",
                axis.key
            );
            let share: f64 = axis.groups.iter().map(|group| group.fraction).sum();
            assert!(
                (share - 1.0).abs() < 1e-9,
                "axis {} sums to {share}",
                axis.key
            );
        }
    }

    #[test]
    fn the_counts_match_the_combinatorics() {
        let result = full();
        // 13 ranks choose 3, times 4^3 suit choices.
        assert_eq!(result.group("pairing", "unpaired").unwrap().flops, 286 * 64);
        // 13 ranks for the trips, times 4 choose 3 suits.
        assert_eq!(result.group("pairing", "trips").unwrap().flops, 13 * 4);
        // Paired flops make up the rest, split by whether the pair is the high card.
        let top = result.group("pairing", "paired-top").unwrap().flops;
        let bottom = result.group("pairing", "paired-bottom").unwrap().flops;
        assert_eq!(top + bottom, 22_100 - 286 * 64 - 13 * 4);
        assert_eq!(
            top, bottom,
            "a pair is as likely to be the high card as the low"
        );

        // Four suits choose 3 arrangements out of 4^3 total.
        assert_eq!(result.group("suits", "monotone").unwrap().flops, 4 * 286);
        let rainbow = result.group("suits", "rainbow").unwrap().fraction;
        assert!(
            (rainbow - 0.3976).abs() < 0.002,
            "rainbow came out at {rainbow:.4}"
        );
        let monotone = result.group("suits", "monotone").unwrap().fraction;
        assert!(
            (monotone - 0.0518).abs() < 0.002,
            "monotone came out at {monotone:.4}"
        );
    }

    #[test]
    fn ace_high_flops_are_about_a_fifth_of_them() {
        let result = full();
        let ace = result.group("high-card", "A").unwrap().fraction;
        assert!(
            (0.17..0.22).contains(&ace),
            "ace-high flops came out at {ace:.4}"
        );
    }

    #[test]
    fn holding_the_aces_makes_ace_high_flops_rarer() {
        let open = full();
        let blocked = breakdown(CardSet::parse("As Ah Ad Ac").unwrap());
        assert_eq!(blocked.group("high-card", "A").unwrap().flops, 0);
        assert!(open.group("high-card", "A").unwrap().flops > 0);
        // Forty-eight cards leave C(48,3) flops.
        assert_eq!(blocked.total, 17_296);
    }

    #[test]
    fn blockers_move_the_numbers() {
        let open = full();
        let with_ak = breakdown(CardSet::parse("As Kd").unwrap());
        let before = open.group("high-card", "A").unwrap().fraction;
        let after = with_ak.group("high-card", "A").unwrap().fraction;
        assert!(
            after < before,
            "holding an ace should make ace-high flops rarer"
        );
        assert!(
            (before - after) > 0.01,
            "the shift should be visible: {before:.4} to {after:.4}"
        );
    }

    #[test]
    fn the_wheel_counts_as_connected() {
        // A23 is three to a straight through the wheel.
        assert_eq!(connectedness(&[12, 1, 0]), 0);
        // A nine between them reaches neither end, but the ace and the deuce are
        // still two to a straight through the wheel.
        assert_eq!(connectedness(&[12, 7, 0]), 1);
        // Nothing within reach of anything.
        assert_eq!(connectedness(&[11, 6, 1]), 2);
        assert_eq!(connectedness(&[10, 9, 8]), 0);
        assert_eq!(connectedness(&[10, 9, 0]), 1);
    }
}
