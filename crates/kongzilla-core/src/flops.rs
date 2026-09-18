//! How often each kind of flop comes.
//!
//! Flopzilla's flop breakdown tool, which answers questions the statistics panel
//! cannot: how often the flop is paired, monotone, ace-high, connected - and how
//! much your own two cards change those numbers. Blockers matter more than people
//! expect: ace-high flops are common until you are the one holding an ace.

use crate::board::Board;
use crate::cards::{CardSet, RANK_CHARS};
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
    let mut pairing_counts = [0u64; 4]; // unpaired, paired top, paired bottom, trips
    let mut suit_counts = [0u64; 3]; // rainbow, two-tone, monotone
    let mut high_counts = [0u64; 6]; // A, K, Q, J, T, nine or lower
    let mut connect_counts = [0u64; 3]; // three, two, none
    let mut total = 0u64;

    for flop in Board::all_flops() {
        if flop.mask().intersects(dead) {
            continue;
        }
        total += 1;

        let ranks = flop.ranks_desc();
        let counts = flop.rank_counts();
        match ranks.len() {
            1 => pairing_counts[3] += 1,
            2 => {
                // The doubled rank is either the higher or the lower of the two.
                if counts[ranks[0] as usize] == 2 {
                    pairing_counts[1] += 1;
                } else {
                    pairing_counts[2] += 1;
                }
            }
            _ => pairing_counts[0] += 1,
        }

        let suits = flop.suit_counts();
        let distinct = suits.iter().filter(|count| **count > 0).count();
        suit_counts[3 - distinct] += 1;

        let high = ranks[0];
        high_counts[match high {
            12 => 0,
            11 => 1,
            10 => 2,
            9 => 3,
            8 => 4,
            _ => 5,
        }] += 1;

        connect_counts[connectedness(&ranks)] += 1;
    }

    let share = |count: u64| {
        if total > 0 {
            count as f64 / total as f64
        } else {
            0.0
        }
    };
    let group = |key: &str, label: &str, count: u64| FlopGroup {
        key: key.to_owned(),
        label: label.to_owned(),
        flops: count,
        fraction: share(count),
    };

    let mut high_groups: Vec<FlopGroup> = Vec::with_capacity(6);
    for (position, rank) in [12u8, 11, 10, 9, 8].into_iter().enumerate() {
        let name = RANK_CHARS[rank as usize] as char;
        high_groups.push(group(
            &name.to_string(),
            &format!("{name} high"),
            high_counts[position],
        ));
    }
    high_groups.push(group("low", "9 high or lower", high_counts[5]));

    FlopBreakdown {
        total,
        axes: vec![
            FlopAxis {
                key: "pairing",
                label: "Pairing",
                groups: vec![
                    group("unpaired", "Unpaired", pairing_counts[0]),
                    group("paired-top", "Paired, top card", pairing_counts[1]),
                    group("paired-bottom", "Paired, bottom card", pairing_counts[2]),
                    group("trips", "Trips", pairing_counts[3]),
                ],
            },
            FlopAxis {
                key: "suits",
                label: "Suits",
                groups: vec![
                    group("rainbow", "Rainbow", suit_counts[0]),
                    group("two-tone", "Two-tone", suit_counts[1]),
                    group("monotone", "Monotone", suit_counts[2]),
                ],
            },
            FlopAxis {
                key: "high-card",
                label: "High card",
                groups: high_groups,
            },
            FlopAxis {
                key: "connectedness",
                label: "Connectedness",
                groups: vec![
                    group("three", "Three to a straight", connect_counts[0]),
                    group("two", "Two to a straight", connect_counts[1]),
                    group("none", "Disconnected", connect_counts[2]),
                ],
            },
        ],
    }
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
