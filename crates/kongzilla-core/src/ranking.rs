//! Hand-strength orderings, which is what the range slider means by "top X%".

use crate::cards::{HandClass, NUM_CLASSES, RANK_ACE};
use crate::ranking_table;

/// The orderings the slider can use.
///
/// Only one ships today; the enum exists so adding Sklansky-Chubukov or a
/// user-supplied chart later does not change any call site.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Ranking {
    /// All-in equity against a uniformly random hand.
    ///
    /// Ranks pocket pairs very highly, because all-in equity is all this metric
    /// knows; it says nothing about how a hand plays once there is betting.
    #[default]
    EquityVsRandom,
    /// Bill Chen's scoring formula.
    ///
    /// A playability heuristic rather than an equity measure: it pays for high
    /// cards, adds two points for suitedness and subtracts for gaps, so suited
    /// connectors climb and small pairs fall.
    ChenFormula,
}

impl Ranking {
    /// Every ordering, for populating a dropdown.
    pub const ALL: [Ranking; 2] = [Ranking::EquityVsRandom, Ranking::ChenFormula];

    /// A stable identifier for serialisation.
    pub const fn key(self) -> &'static str {
        match self {
            Self::EquityVsRandom => "equity-vs-random",
            Self::ChenFormula => "chen",
        }
    }

    /// The name shown in the interface.
    pub const fn label(self) -> &'static str {
        match self {
            Self::EquityVsRandom => "Equity vs random",
            Self::ChenFormula => "Chen formula",
        }
    }

    /// Reads a [`Ranking::key`].
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.key() == key)
    }

    /// The 169 matrix cells, strongest first.
    pub fn order(self) -> &'static [HandClass; NUM_CLASSES] {
        match self {
            Self::EquityVsRandom => &ranking_table::EQUITY_VS_RANDOM,
            Self::ChenFormula => chen_order(),
        }
    }

    /// Where a cell sits in the ordering, `0` being the strongest.
    pub fn position(self, class: HandClass) -> usize {
        self.order()
            .iter()
            .position(|c| *c == class)
            .expect("every class appears in every ordering")
    }
}

/// Bill Chen's score for a starting hand.
///
/// High card value, doubled for a pair, plus two for suitedness, minus a penalty
/// for the gap, plus a point for a low connector - then rounded up, as published.
pub fn chen_score(class: HandClass) -> f32 {
    let value = |rank: u8| -> f32 {
        match rank {
            RANK_ACE => 10.0,
            11 => 8.0,
            10 => 7.0,
            9 => 6.0,
            other => (f32::from(other) + 2.0) / 2.0,
        }
    };

    let (high, low) = (class.high_rank(), class.low_rank());
    if class.is_pair() {
        return (value(high) * 2.0).max(5.0);
    }

    let mut score = value(high);
    if class.is_suited() {
        score += 2.0;
    }
    let gap = high - low - 1;
    score -= match gap {
        0 => 0.0,
        1 => 1.0,
        2 => 2.0,
        3 => 4.0,
        _ => 5.0,
    };
    // A point back for connected low cards, which make straights.
    if gap <= 1 && high < 10 {
        score += 1.0;
    }
    score.ceil()
}

/// The Chen ordering, computed once. Ties fall back to equity against a random
/// hand so the order is total and stable.
fn chen_order() -> &'static [HandClass; NUM_CLASSES] {
    static ORDER: std::sync::OnceLock<[HandClass; NUM_CLASSES]> = std::sync::OnceLock::new();
    ORDER.get_or_init(|| {
        let mut classes = ranking_table::EQUITY_VS_RANDOM;
        classes.sort_by(|a, b| {
            chen_score(*b)
                .partial_cmp(&chen_score(*a))
                .expect("Chen scores are finite")
        });
        classes
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ordering_is_a_permutation() {
        for ranking in Ranking::ALL {
            let mut seen = [false; NUM_CLASSES];
            for class in ranking.order() {
                assert!(!seen[class.index() as usize], "{class} listed twice");
                seen[class.index() as usize] = true;
            }
            assert!(seen.into_iter().all(|s| s));
        }
    }

    #[test]
    fn the_ordering_is_sane() {
        let order = Ranking::EquityVsRandom.order();
        assert_eq!(order[0].to_string(), "AA");
        let position =
            |hand: &str| Ranking::EquityVsRandom.position(HandClass::parse(hand).unwrap());
        assert!(position("AA") < position("KK"));
        assert!(position("KK") < position("QQ"));
        assert!(position("QQ") < position("22"));
        assert!(position("AKs") < position("AKo"));
        assert!(position("AKs") < position("A2s"));
        assert!(position("AKo") < position("72o"));
        assert!(position("72o") > 150, "72o should sit near the bottom");
    }

    #[test]
    fn chen_scores_match_the_published_formula() {
        let score = |hand: &str| chen_score(HandClass::parse(hand).unwrap());
        assert_eq!(score("AA"), 20.0);
        assert_eq!(score("KK"), 16.0);
        assert_eq!(score("22"), 5.0, "small pairs floor at five");
        assert_eq!(score("AKs"), 12.0);
        assert_eq!(score("AKo"), 10.0);
        assert_eq!(score("AQs"), 11.0, "one gap costs a point");
        assert_eq!(
            score("65s"),
            6.0,
            "three for the six, two for suited, one for connected"
        );
        assert!(score("72o") < score("22"));
    }

    #[test]
    fn chen_values_playability_differently_from_raw_equity() {
        let chen = |hand: &str| Ranking::ChenFormula.position(HandClass::parse(hand).unwrap());
        let equity = |hand: &str| Ranking::EquityVsRandom.position(HandClass::parse(hand).unwrap());
        assert_eq!(Ranking::ChenFormula.order()[0].to_string(), "AA");
        // Suited connectors climb under Chen and small pairs fall.
        assert!(chen("65s") < chen("22"));
        assert!(equity("65s") > equity("22"));
    }

    #[test]
    fn keys_round_trip() {
        for ranking in Ranking::ALL {
            assert_eq!(Ranking::from_key(ranking.key()), Some(ranking));
        }
        assert_eq!(Ranking::from_key("nope"), None);
    }
}
