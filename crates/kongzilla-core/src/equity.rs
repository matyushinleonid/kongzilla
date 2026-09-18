//! All-in equity between ranges.
//!
//! Postflop equities are enumerated exactly. The inner loop avoids the naive
//! `hero x villain x runout` triple by sorting each runout's hands by strength and
//! walking the two sorted lists together, correcting for card removal as it goes;
//! that turns a cubic loop into something close to linear in the number of combos.
//!
//! Preflop there are 2.1 million runouts, so the estimate switches to Monte Carlo
//! with a fixed seed, which keeps repeated calls reproducible.

use crate::board::Board;
use crate::cards::{Card, CardSet, Combo, NUM_CARDS, NUM_COMBOS};
use crate::eval::eval;
use crate::range::Range;
use crate::rng::Rng;

/// How often a player wins, ties, and their resulting share of the pot.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Equity {
    /// Share of run-outs won outright, in `0.0..=1.0`.
    pub win: f64,
    /// Share of run-outs split, in `0.0..=1.0`.
    pub tie: f64,
    /// Pot share: wins plus an even split of the ties.
    pub equity: f64,
}

/// The result of an equity calculation.
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct EquityReport {
    /// One entry per player, in the order they were given.
    pub players: Vec<Equity>,
    /// Whether every run-out was enumerated.
    pub exact: bool,
    /// How many run-outs or samples were evaluated.
    pub trials: u64,
}

/// How many samples a preflop estimate draws.
pub const PREFLOP_SAMPLES: u32 = 200_000;

/// How many samples a multiway estimate draws once the board is out.
///
/// Fewer than a preflop estimate needs, because most of the board is already
/// known: the only thing left to sample is the hands and the cards to come.
pub const MULTIWAY_SAMPLES: u32 = 100_000;

/// The seed used for Monte Carlo estimates, so results are reproducible.
pub const MONTE_CARLO_SEED: u64 = 0x4B6F_6E67_7A69_6C6C;

#[derive(Clone, Copy)]
struct Entry {
    rank: u32,
    weight: f64,
    c0: usize,
    c1: usize,
    combo: u16,
}

/// Equity for each of the first range's combos.
///
/// This is what the equity matrix paints onto the starting-hand grid and what the
/// equity graph sorts into a curve. The enumeration already works one hero hand at
/// a time, so keeping the per-hand numbers costs nothing beyond the arrays.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ComboEquity {
    /// Equity per combo index, or `-1.0` where the combo is not in the range.
    pub equity: Vec<f32>,
    /// Weight per combo index, so the graph can size each point.
    pub weight: Vec<f32>,
    /// Share of run-outs won outright, per combo.
    pub win: Vec<f32>,
    /// Share of run-outs split, per combo.
    pub tie: Vec<f32>,
}

/// How one turn card changes a hand's equity.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct HotCard {
    /// The card, as text.
    pub card: &'static str,
    /// The hand's equity once that card lands.
    pub equity: f64,
}

struct Enumerated {
    report: EquityReport,
    numerator: Box<[f64; NUM_COMBOS]>,
    denominator: Box<[f64; NUM_COMBOS]>,
    /// Outright wins and splits per combo, kept apart so a table can show both.
    wins: Box<[f64; NUM_COMBOS]>,
    ties: Box<[f64; NUM_COMBOS]>,
}

/// Equity of one specific hand against a range.
pub fn hand_vs_range(
    hero: Combo,
    villain: &Range,
    board: &Board,
    dead: CardSet,
) -> Option<EquityReport> {
    let mut hero_range = Range::empty();
    hero_range.set(hero, 1.0);
    range_vs_range(&hero_range, villain, board, dead.difference(hero.mask()))
}

/// Equity between two ranges.
///
/// Returns `None` when either range has nothing playable left after the board and
/// the dead cards are removed.
pub fn range_vs_range(a: &Range, b: &Range, board: &Board, dead: CardSet) -> Option<EquityReport> {
    range_vs_ranges(&[a, b], board, dead)
}

/// Equity between any number of ranges, in the order they are given.
///
/// Two ranges on a dealt board are enumerated exactly. Anything else is sampled,
/// and for two different reasons. Preflop there are 2.1 million run-outs. And
/// with three or more players the pot goes to the best of them all at once,
/// which the merge of two sorted lists above cannot be made to answer: whether
/// one hand wins depends on every other hand in the pot, not on one opponent at
/// a time. A fixed seed keeps a sampled answer reproducible, and the report says
/// which kind of answer it is.
pub fn range_vs_ranges(ranges: &[&Range], board: &Board, dead: CardSet) -> Option<EquityReport> {
    if ranges.len() < 2 || !board.is_dealt() {
        return None;
    }
    let blocked = board.mask().union(dead);
    let lives: Vec<Vec<(Combo, f32)>> = ranges.iter().map(|r| r.live(blocked)).collect();
    if lives.iter().any(Vec::is_empty) {
        return None;
    }

    match (lives.len(), board.is_empty()) {
        (2, false) => Some(enumerate(&lives[0], &lives[1], board, blocked).report),
        (2, true) => Some(monte_carlo(&lives[0], &lives[1], blocked, PREFLOP_SAMPLES)),
        _ => Some(monte_carlo_many(
            &lives,
            board,
            blocked,
            if board.is_empty() {
                PREFLOP_SAMPLES
            } else {
                MULTIWAY_SAMPLES
            },
        )),
    }
}

/// Equity for each of the first range's combos.
///
/// Returns `None` preflop, where the estimate is sampled and per-hand numbers
/// would be far too noisy to paint.
pub fn equity_by_combo(a: &Range, b: &Range, board: &Board, dead: CardSet) -> Option<ComboEquity> {
    if board.is_empty() || !board.is_dealt() {
        return None;
    }
    let blocked = board.mask().union(dead);
    let a_live = a.live(blocked);
    let b_live = b.live(blocked);
    if a_live.is_empty() || b_live.is_empty() {
        return None;
    }

    let result = enumerate(&a_live, &b_live, board, blocked);
    let mut equity = vec![-1.0f32; NUM_COMBOS];
    let mut weight = vec![0.0f32; NUM_COMBOS];
    let mut win = vec![0.0f32; NUM_COMBOS];
    let mut tie = vec![0.0f32; NUM_COMBOS];
    for (combo, hand_weight) in &a_live {
        let index = combo.index() as usize;
        let runouts = result.denominator[index];
        if runouts > 0.0 {
            equity[index] = (result.numerator[index] / runouts) as f32;
            weight[index] = *hand_weight;
            win[index] = (result.wins[index] / runouts) as f32;
            tie[index] = (result.ties[index] / runouts) as f32;
        }
    }
    Some(ComboEquity {
        equity,
        weight,
        win,
        tie,
    })
}

/// How each remaining card would change a hand's equity against a range.
///
/// Flopzilla calls this hotness and colours the deck with it: red where the card
/// helps the range, green where it helps the hand. Needs a flop or a turn, so
/// there is a card still to come.
pub fn hotness(hero: Combo, villain: &Range, board: &Board, dead: CardSet) -> Option<Vec<HotCard>> {
    if board.len() < 3 || board.len() >= Board::MAX {
        return None;
    }
    let used = board.mask().union(dead).union(hero.mask());
    let mut cards: Vec<HotCard> = Vec::with_capacity(NUM_CARDS);
    for card in CardSet::FULL.difference(used).iter() {
        let Some(next) = board.with_card(card) else {
            continue;
        };
        if let Some(report) = hand_vs_range(hero, villain, &next, dead) {
            cards.push(HotCard {
                card: card_name(card),
                equity: report.players[0].equity,
            });
        }
    }
    (!cards.is_empty()).then_some(cards)
}

/// Card names as static strings, so the report can be serialised without
/// allocating a string per card.
fn card_name(card: Card) -> &'static str {
    static NAMES: [&str; NUM_CARDS] = [
        "2c", "2d", "2h", "2s", "3c", "3d", "3h", "3s", "4c", "4d", "4h", "4s", "5c", "5d", "5h",
        "5s", "6c", "6d", "6h", "6s", "7c", "7d", "7h", "7s", "8c", "8d", "8h", "8s", "9c", "9d",
        "9h", "9s", "Tc", "Td", "Th", "Ts", "Jc", "Jd", "Jh", "Js", "Qc", "Qd", "Qh", "Qs", "Kc",
        "Kd", "Kh", "Ks", "Ac", "Ad", "Ah", "As",
    ];
    NAMES[card.index() as usize]
}

fn enumerate(
    a_live: &[(Combo, f32)],
    b_live: &[(Combo, f32)],
    board: &Board,
    blocked: CardSet,
) -> Enumerated {
    let deck: Vec<Card> = CardSet::FULL.difference(blocked).iter().collect();
    let to_come = Board::MAX - board.len();
    let board_mask = board.mask();

    let mut a_equity = 0.0f64;
    let mut b_equity = 0.0f64;
    let mut a_win = 0.0f64;
    let mut a_tie = 0.0f64;
    let mut b_win = 0.0f64;
    let mut total_weight = 0.0f64;
    let mut trials = 0u64;
    let mut numerator = Box::new([0.0f64; NUM_COMBOS]);
    let mut denominator = Box::new([0.0f64; NUM_COMBOS]);
    let mut wins = Box::new([0.0f64; NUM_COMBOS]);
    let mut ties = Box::new([0.0f64; NUM_COMBOS]);

    let mut visit = |runout: CardSet| {
        trials += 1;
        let full = board_mask.union(runout);
        let used = blocked.union(runout);

        let mut a_entries = collect(a_live, full, used);
        let mut b_entries = collect(b_live, full, used);
        if a_entries.is_empty() || b_entries.is_empty() {
            return;
        }
        a_entries.sort_unstable_by_key(|e| e.rank);
        b_entries.sort_unstable_by_key(|e| e.rank);

        let mut card_weight = [0.0f64; NUM_CARDS];
        let mut b_total = 0.0f64;
        for entry in &b_entries {
            card_weight[entry.c0] += entry.weight;
            card_weight[entry.c1] += entry.weight;
            b_total += entry.weight;
        }

        let mut cursor = 0usize;
        let mut lt_weight = 0.0f64;
        let mut lt_card = [0.0f64; NUM_CARDS];

        for hero in &a_entries {
            while cursor < b_entries.len() && b_entries[cursor].rank < hero.rank {
                let entry = b_entries[cursor];
                lt_weight += entry.weight;
                lt_card[entry.c0] += entry.weight;
                lt_card[entry.c1] += entry.weight;
                cursor += 1;
            }

            let mut tie_weight = 0.0f64;
            let mut tie_conflict = 0.0f64;
            let mut same_combo = 0.0f64;
            let mut scan = cursor;
            while scan < b_entries.len() && b_entries[scan].rank == hero.rank {
                let entry = b_entries[scan];
                tie_weight += entry.weight;
                let touches_hero = entry.c0 == hero.c0
                    || entry.c0 == hero.c1
                    || entry.c1 == hero.c0
                    || entry.c1 == hero.c1;
                if touches_hero {
                    tie_conflict += entry.weight;
                }
                if entry.combo == hero.combo {
                    same_combo += entry.weight;
                }
                scan += 1;
            }

            // Villain hands that clash with the hero's cards never get dealt.
            let win = lt_weight - lt_card[hero.c0] - lt_card[hero.c1];
            let tie = tie_weight - tie_conflict;
            let hero_denominator =
                b_total - card_weight[hero.c0] - card_weight[hero.c1] + same_combo;
            if hero_denominator <= 0.0 {
                continue;
            }

            total_weight += hero.weight * hero_denominator;
            a_win += hero.weight * win;
            a_tie += hero.weight * tie;
            b_win += hero.weight * (hero_denominator - win - tie);
            a_equity += hero.weight * (win + 0.5 * tie);
            b_equity += hero.weight * (hero_denominator - win - 0.5 * tie);
            numerator[hero.combo as usize] += win + 0.5 * tie;
            denominator[hero.combo as usize] += hero_denominator;
            wins[hero.combo as usize] += win;
            ties[hero.combo as usize] += tie;
        }
    };

    match to_come {
        0 => visit(CardSet::EMPTY),
        1 => {
            for card in &deck {
                visit(card.mask());
            }
        }
        2 => {
            for i in 0..deck.len() {
                for j in i + 1..deck.len() {
                    visit(deck[i].mask().union(deck[j].mask()));
                }
            }
        }
        _ => unreachable!("a dealt board has three to five cards"),
    }

    let report = if total_weight <= 0.0 {
        EquityReport {
            players: vec![Equity::default(); 2],
            exact: true,
            trials,
        }
    } else {
        EquityReport {
            players: vec![
                Equity {
                    win: a_win / total_weight,
                    tie: a_tie / total_weight,
                    equity: a_equity / total_weight,
                },
                Equity {
                    win: b_win / total_weight,
                    tie: a_tie / total_weight,
                    equity: b_equity / total_weight,
                },
            ],
            exact: true,
            trials,
        }
    };

    Enumerated {
        report,
        numerator,
        denominator,
        wins,
        ties,
    }
}

fn collect(live: &[(Combo, f32)], full_board: CardSet, used: CardSet) -> Vec<Entry> {
    live.iter()
        .filter(|(combo, _)| !combo.mask().intersects(used))
        .map(|(combo, weight)| {
            let (c0, c1) = combo.cards();
            Entry {
                rank: eval(full_board.union(combo.mask())).value(),
                weight: f64::from(*weight),
                c0: c0.index() as usize,
                c1: c1.index() as usize,
                combo: combo.index(),
            }
        })
        .collect()
}

fn monte_carlo(
    a_live: &[(Combo, f32)],
    b_live: &[(Combo, f32)],
    blocked: CardSet,
    samples: u32,
) -> EquityReport {
    let a_pick = Picker::new(a_live);
    let b_pick = Picker::new(b_live);
    let deck: Vec<Card> = CardSet::FULL.difference(blocked).iter().collect();
    let mut rng = Rng::new(MONTE_CARLO_SEED);

    let mut a_win = 0.0f64;
    let mut ties = 0.0f64;
    let mut b_win = 0.0f64;
    let mut trials = 0u64;

    let mut attempts = 0u64;
    let cap = u64::from(samples) * 20;
    while trials < u64::from(samples) && attempts < cap {
        attempts += 1;
        let hero = a_pick.sample(&mut rng);
        let villain = b_pick.sample(&mut rng);
        if hero.mask().intersects(villain.mask()) {
            continue;
        }
        let mut used = blocked.union(hero.mask()).union(villain.mask());
        let mut runout = CardSet::EMPTY;
        let mut drawn = 0;
        while drawn < Board::MAX {
            let card = deck[rng.below(deck.len() as u32) as usize];
            if used.contains(card) {
                continue;
            }
            used.insert(card);
            runout.insert(card);
            drawn += 1;
        }
        let hero_rank = eval(runout.union(hero.mask()));
        let villain_rank = eval(runout.union(villain.mask()));
        match hero_rank.cmp(&villain_rank) {
            core::cmp::Ordering::Greater => a_win += 1.0,
            core::cmp::Ordering::Equal => ties += 1.0,
            core::cmp::Ordering::Less => b_win += 1.0,
        }
        trials += 1;
    }

    if trials == 0 {
        return EquityReport {
            players: vec![Equity::default(); 2],
            exact: false,
            trials: 0,
        };
    }

    let total = trials as f64;
    EquityReport {
        players: vec![
            Equity {
                win: a_win / total,
                tie: ties / total,
                equity: (a_win + 0.5 * ties) / total,
            },
            Equity {
                win: b_win / total,
                tie: ties / total,
                equity: (b_win + 0.5 * ties) / total,
            },
        ],
        exact: false,
        trials,
    }
}

/// Equity among three or more players, by sampling.
///
/// One hand is dealt to each player and the board is filled in; the best hand
/// takes the pot, and a tied pot is split among everyone who holds the best.
/// Deals where two players would need the same card are thrown away rather than
/// corrected for, which is what makes the card removal come out right.
fn monte_carlo_many(
    lives: &[Vec<(Combo, f32)>],
    board: &Board,
    blocked: CardSet,
    samples: u32,
) -> EquityReport {
    let pickers: Vec<Picker> = lives.iter().map(|live| Picker::new(live)).collect();
    let deck: Vec<Card> = CardSet::FULL.difference(blocked).iter().collect();
    let to_come = Board::MAX - board.len();
    let board_mask = board.mask();
    let mut rng = Rng::new(MONTE_CARLO_SEED);

    let seats = lives.len();
    let mut wins = vec![0.0f64; seats];
    let mut ties = vec![0.0f64; seats];
    let mut shares = vec![0.0f64; seats];
    let mut hands: Vec<Combo> = Vec::with_capacity(seats);
    let mut trials = 0u64;

    let mut attempts = 0u64;
    let cap = u64::from(samples) * 40;
    while trials < u64::from(samples) && attempts < cap {
        attempts += 1;

        hands.clear();
        let mut used = blocked;
        let mut clash = false;
        for picker in &pickers {
            let hand = picker.sample(&mut rng);
            if hand.mask().intersects(used) {
                clash = true;
                break;
            }
            used = used.union(hand.mask());
            hands.push(hand);
        }
        if clash {
            continue;
        }

        let mut runout = CardSet::EMPTY;
        let mut drawn = 0;
        while drawn < to_come {
            let card = deck[rng.below(deck.len() as u32) as usize];
            if used.contains(card) {
                continue;
            }
            used.insert(card);
            runout.insert(card);
            drawn += 1;
        }

        let full = board_mask.union(runout);
        let mut best = 0u32;
        let mut winners = 0usize;
        for hand in &hands {
            let rank = eval(full.union(hand.mask())).value();
            if rank > best {
                best = rank;
                winners = 1;
            } else if rank == best {
                winners += 1;
            }
        }

        let split = 1.0 / winners as f64;
        for (seat, hand) in hands.iter().enumerate() {
            if eval(full.union(hand.mask())).value() != best {
                continue;
            }
            if winners == 1 {
                wins[seat] += 1.0;
            } else {
                ties[seat] += 1.0;
            }
            shares[seat] += split;
        }
        trials += 1;
    }

    if trials == 0 {
        return EquityReport {
            players: vec![Equity::default(); seats],
            exact: false,
            trials: 0,
        };
    }

    let total = trials as f64;
    EquityReport {
        players: (0..seats)
            .map(|seat| Equity {
                win: wins[seat] / total,
                tie: ties[seat] / total,
                equity: shares[seat] / total,
            })
            .collect(),
        exact: false,
        trials,
    }
}

/// Weighted sampling over the combos of a range.
struct Picker {
    combos: Vec<Combo>,
    cumulative: Vec<f64>,
    total: f64,
}

impl Picker {
    fn new(live: &[(Combo, f32)]) -> Self {
        let mut combos = Vec::with_capacity(live.len());
        let mut cumulative = Vec::with_capacity(live.len());
        let mut total = 0.0;
        for (combo, weight) in live {
            total += f64::from(*weight);
            combos.push(*combo);
            cumulative.push(total);
        }
        Self {
            combos,
            cumulative,
            total,
        }
    }

    fn sample(&self, rng: &mut Rng) -> Combo {
        let target = rng.unit() * self.total;
        match self
            .cumulative
            .binary_search_by(|value| value.partial_cmp(&target).expect("no NaN weights"))
        {
            Ok(index) => self.combos[index.min(self.combos.len() - 1)],
            Err(index) => self.combos[index.min(self.combos.len() - 1)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn equity_of(hero: &str, villain: &str, board: &str) -> f64 {
        let board = Board::parse(board).unwrap();
        let report = range_vs_range(
            &Range::parse(hero).unwrap(),
            &Range::parse(villain).unwrap(),
            &board,
            CardSet::EMPTY,
        )
        .unwrap();
        report.players[0].equity
    }

    fn three_way(a: &str, b: &str, c: &str, board: &str) -> Vec<f64> {
        let board = Board::parse(board).unwrap();
        let (a, b, c) = (
            Range::parse(a).unwrap(),
            Range::parse(b).unwrap(),
            Range::parse(c).unwrap(),
        );
        let report = range_vs_ranges(&[&a, &b, &c], &board, CardSet::EMPTY).unwrap();
        assert!(!report.exact, "three players are sampled, not enumerated");
        report.players.iter().map(|p| p.equity).collect()
    }

    #[test]
    fn a_three_way_pot_shares_out_whole() {
        // Everyone holds the same range, so everyone has a third of the pot.
        let shares = three_way("22+,A2s+", "22+,A2s+", "22+,A2s+", "Kh 7d 2c");
        for share in &shares {
            assert!(
                (share - 1.0 / 3.0).abs() < 0.01,
                "an even three-way pot: {shares:?}"
            );
        }
        assert!(
            (shares.iter().sum::<f64>() - 1.0).abs() < 1e-9,
            "{shares:?}"
        );
    }

    #[test]
    fn a_locked_hand_beats_two_of_them() {
        // Quads against two hands that cannot get there by the river.
        let shares = three_way("7c7d", "AhKh", "AsQs", "7h 7s 2c");
        assert!(shares[0] > 0.999, "quads are not losing this: {shares:?}");
        assert!(shares[1] < 0.001 && shares[2] < 0.001, "{shares:?}");
    }

    /// A third player in the pot takes share off the other two here.
    ///
    /// Not a law: equity is a share of one pot, and the third player's two
    /// cards also come out of the deck, which thins the run-outs. A hand whose
    /// outs are untouched can come out of the deal with a slightly *larger*
    /// share than it had heads-up. This is one spot where both do give up
    /// share, and the pot staying whole is the part that always holds.
    #[test]
    fn a_third_player_takes_equity_from_both() {
        let board = Board::parse("Kh 7d 2c").unwrap();
        let (a, b, c) = (
            Range::parse("AhKs").unwrap(),
            Range::parse("QsQd").unwrap(),
            Range::parse("7h6h").unwrap(),
        );
        let heads_up = range_vs_ranges(&[&a, &b], &board, CardSet::EMPTY).unwrap();
        let three = range_vs_ranges(&[&a, &b, &c], &board, CardSet::EMPTY).unwrap();
        assert!(heads_up.exact, "two players on a board are enumerated");
        // Nobody gains from another player entering the pot.
        for seat in 0..2 {
            assert!(
                three.players[seat].equity < heads_up.players[seat].equity,
                "seat {seat}: {:?} vs {:?}",
                three.players[seat],
                heads_up.players[seat]
            );
        }
        let total: f64 = three.players.iter().map(|p| p.equity).sum();
        assert!((total - 1.0).abs() < 1e-9, "the pot is one pot: {total}");
    }

    #[test]
    fn sampling_the_same_spot_twice_gives_the_same_answer() {
        let first = three_way("JJ", "AKs", "T9s", "Qh 8d 3c");
        let second = three_way("JJ", "AKs", "T9s", "Qh 8d 3c");
        assert_eq!(first, second, "a fixed seed means a reproducible answer");
    }

    #[test]
    fn a_locked_hand_has_all_the_equity() {
        // Quads against a range that cannot beat them.
        let equity = equity_of("7c7d", "AhKh,AsKs", "7h 7s 2c");
        assert!((equity - 1.0).abs() < 1e-9, "expected 100%, got {equity}");
    }

    #[test]
    fn a_dead_hand_has_none() {
        let equity = equity_of("AhKh", "7c7d", "7h 7s 2c 3d 4s");
        assert!(equity.abs() < 1e-9, "expected 0%, got {equity}");
    }

    #[test]
    fn identical_hands_chop() {
        // Both players play the same board, so every run-out is a split.
        let board = Board::parse("Ah Kh Qh Jh Th").unwrap();
        let report = range_vs_range(
            &Range::parse("2c2d").unwrap(),
            &Range::parse("3c3d").unwrap(),
            &board,
            CardSet::EMPTY,
        )
        .unwrap();
        assert!((report.players[0].equity - 0.5).abs() < 1e-9);
        assert!((report.players[0].tie - 1.0).abs() < 1e-9);
    }

    #[test]
    fn equities_sum_to_one() {
        for (hero, villain, board) in [
            ("AhKh", "QQ", "Kc 7d 2s"),
            ("22+,A2s+", "KQs,KJs,QJs", "Ts 9d 2c"),
            ("AcKd", "JJ", "Qh 7c 3d 8s"),
            ("AcKd", "JJ", "Qh 7c 3d 8s 2h"),
        ] {
            let board = Board::parse(board).unwrap();
            let report = range_vs_range(
                &Range::parse(hero).unwrap(),
                &Range::parse(villain).unwrap(),
                &board,
                CardSet::EMPTY,
            )
            .unwrap();
            let sum = report.players[0].equity + report.players[1].equity;
            assert!(
                (sum - 1.0).abs() < 1e-6,
                "{hero} vs {villain}: sum was {sum}"
            );
            assert!(report.exact);
        }
    }

    #[test]
    fn a_hand_versus_a_range_matches_the_range_of_one() {
        let board = Board::parse("Kc 7d 2s").unwrap();
        let hero = Combo::parse("AhKh").unwrap();
        let villain = Range::parse("QQ,JJ,TT").unwrap();
        let direct = hand_vs_range(hero, &villain, &board, CardSet::EMPTY).unwrap();
        let mut as_range = Range::empty();
        as_range.set(hero, 1.0);
        let via_range = range_vs_range(&as_range, &villain, &board, CardSet::EMPTY).unwrap();
        assert!((direct.players[0].equity - via_range.players[0].equity).abs() < 1e-12);
    }

    #[test]
    fn card_removal_is_respected() {
        // One ace on the board leaves three of the six ace combinations.
        let board = Board::parse("Ac 7h 2d").unwrap();
        let villain = Range::parse("AA").unwrap();
        assert_eq!(villain.live(board.mask()).len(), 3);
        let report = hand_vs_range(
            Combo::parse("KcKd").unwrap(),
            &villain,
            &board,
            CardSet::EMPTY,
        )
        .expect("three combinations are still a range");
        assert!(
            report.players[0].equity < 0.2,
            "kings against trip aces should be in bad shape, got {}",
            report.players[0].equity
        );
    }

    #[test]
    fn dead_cards_shrink_the_ranges() {
        let board = Board::parse("Kc 7d 2s").unwrap();
        let hero = Combo::parse("KsKh").unwrap();
        let villain = Range::parse("AA").unwrap();
        let dead = CardSet::parse("As Ad").unwrap();
        assert_eq!(villain.live(board.mask()).len(), 6);
        assert_eq!(villain.live(board.mask().union(dead)).len(), 1);

        let open = hand_vs_range(hero, &villain, &board, CardSet::EMPTY).unwrap();
        let narrowed = hand_vs_range(hero, &villain, &board, dead).unwrap();
        // A set of kings is a big favourite either way, but the numbers differ.
        assert!(open.players[0].equity > 0.8);
        assert!(narrowed.players[0].equity > 0.8);
        assert_ne!(open.players[0].equity, narrowed.players[0].equity);
    }

    #[test]
    fn preflop_falls_back_to_sampling_and_is_reproducible() {
        let empty = Board::empty();
        let first = range_vs_range(
            &Range::parse("AcKd").unwrap(),
            &Range::parse("QQ").unwrap(),
            &empty,
            CardSet::EMPTY,
        )
        .unwrap();
        let second = range_vs_range(
            &Range::parse("AcKd").unwrap(),
            &Range::parse("QQ").unwrap(),
            &empty,
            CardSet::EMPTY,
        )
        .unwrap();
        assert_eq!(first, second, "the same inputs must give the same estimate");
        assert!(!first.exact);
        // AKo against queens is a coin flip a little under 43%.
        assert!(
            (first.players[0].equity - 0.43).abs() < 0.02,
            "AKo vs QQ came out at {}",
            first.players[0].equity
        );
    }

    #[test]
    fn known_postflop_spots_are_close_to_published_numbers() {
        // A flush draw with two overcards against top pair is a small favourite.
        let equity = equity_of("AhQh", "KsQs", "Kh 7h 2c");
        assert!(
            (0.4..0.6).contains(&equity),
            "AhQh vs KsQs on Kh7h2c came out at {equity}"
        );
    }

    /// The obvious cubic loop over hero, villain and run-out.
    ///
    /// Slow, but transparently correct: it is the yardstick the optimised walk in
    /// [`enumerate`] is measured against.
    fn brute_force(a: &Range, b: &Range, board: &Board, dead: CardSet) -> [f64; 2] {
        let blocked = board.mask().union(dead);
        let a_live = a.live(blocked);
        let b_live = b.live(blocked);
        let deck: Vec<Card> = CardSet::FULL.difference(blocked).iter().collect();
        let board_mask = board.mask();

        let mut runouts: Vec<CardSet> = Vec::new();
        match Board::MAX - board.len() {
            0 => runouts.push(CardSet::EMPTY),
            1 => runouts.extend(deck.iter().map(|c| c.mask())),
            2 => {
                for i in 0..deck.len() {
                    for j in i + 1..deck.len() {
                        runouts.push(deck[i].mask().union(deck[j].mask()));
                    }
                }
            }
            _ => panic!("the brute force yardstick only covers dealt boards"),
        }

        let mut equity = [0.0f64; 2];
        let mut total = 0.0f64;
        for runout in runouts {
            let full = board_mask.union(runout);
            for (hero, hero_weight) in &a_live {
                if hero.mask().intersects(runout) {
                    continue;
                }
                for (villain, villain_weight) in &b_live {
                    if villain.mask().intersects(runout) || villain.mask().intersects(hero.mask()) {
                        continue;
                    }
                    let weight = f64::from(*hero_weight) * f64::from(*villain_weight);
                    let hero_rank = eval(full.union(hero.mask()));
                    let villain_rank = eval(full.union(villain.mask()));
                    match hero_rank.cmp(&villain_rank) {
                        core::cmp::Ordering::Greater => equity[0] += weight,
                        core::cmp::Ordering::Equal => {
                            equity[0] += weight * 0.5;
                            equity[1] += weight * 0.5;
                        }
                        core::cmp::Ordering::Less => equity[1] += weight,
                    }
                    total += weight;
                }
            }
        }
        [equity[0] / total, equity[1] / total]
    }

    #[test]
    fn the_fast_path_agrees_with_brute_force() {
        let cases = [
            ("AhKh,AsKs,QcQd", "JJ,TT,AcAd", "Kc 7d 2s", ""),
            ("22+", "AKs,AQs", "Kc Qh Jh", ""),
            ("AhQh,KsQs,7c7d", "AA,KK,QQ,JJ", "Kh 7h 2c Ts", ""),
            ("AcKd:0.5,QQ", "JJ,TT:0.25", "Qh 7c 3d 8s", ""),
            ("AhKh", "AA,KK,QQ", "Kc 7d 2s", "As Ad"),
        ];
        for (hero, villain, board, dead) in cases {
            let hero_range = Range::parse(hero).unwrap();
            let villain_range = Range::parse(villain).unwrap();
            let board = Board::parse(board).unwrap();
            let dead = CardSet::parse(dead).unwrap();

            let fast = range_vs_range(&hero_range, &villain_range, &board, dead).unwrap();
            let slow = brute_force(&hero_range, &villain_range, &board, dead);
            assert!(fast.exact, "{hero} vs {villain} should enumerate");
            assert!(
                (fast.players[0].equity - slow[0]).abs() < 1e-9,
                "{hero} vs {villain} on {board}: fast {}, brute force {}",
                fast.players[0].equity,
                slow[0]
            );
            assert!((fast.players[1].equity - slow[1]).abs() < 1e-9);
        }
    }

    #[test]
    fn a_set_is_about_a_nine_to_one_favourite_over_an_overpair() {
        let equity = equity_of("7c7d", "AcAd", "7h Ks 2c");
        assert!(
            (0.90..0.92).contains(&equity),
            "set over overpair came out at {equity}"
        );
    }

    #[test]
    fn per_combo_equity_averages_back_to_the_summary() {
        let board = Board::parse("Kc 7d 2s").unwrap();
        let hero = Range::parse("AKs,QQ,77").unwrap();
        let villain = Range::parse("JJ,TT,AQs").unwrap();
        let summary = range_vs_range(&hero, &villain, &board, CardSet::EMPTY).unwrap();
        let per_combo = equity_by_combo(&hero, &villain, &board, CardSet::EMPTY).unwrap();

        assert_eq!(per_combo.equity.len(), crate::cards::NUM_COMBOS);
        let mut weighted = 0.0f64;
        let mut total = 0.0f64;
        for (combo, weight) in hero.live(board.mask()) {
            let index = combo.index() as usize;
            let equity = per_combo.equity[index];
            assert!(equity >= 0.0, "{combo} should have equity");
            assert!((per_combo.weight[index] - weight).abs() < 1e-6);
            weighted += f64::from(equity) * f64::from(weight);
            total += f64::from(weight);
        }
        // Each hand is dealt equally often, so the plain weighted mean matches.
        assert!(
            (weighted / total - summary.players[0].equity).abs() < 1e-3,
            "{} vs {}",
            weighted / total,
            summary.players[0].equity
        );

        // Trip sevens are far ahead of a hand that only has top pair. The seven
        // of diamonds is on the board, so the live pair is the other two.
        let trips = per_combo.equity[Combo::parse("7h7s").unwrap().index() as usize];
        let top_pair = per_combo.equity[Combo::parse("AsKs").unwrap().index() as usize];
        assert!(
            trips > top_pair,
            "trips beat top pair: {trips} vs {top_pair}"
        );
        // Combos outside the range are marked, not zero.
        assert_eq!(
            per_combo.equity[Combo::parse("2c3d").unwrap().index() as usize],
            -1.0
        );
    }

    #[test]
    fn hotness_ranks_the_cards_the_hand_wants() {
        let board = Board::parse("Kh 7h 2c").unwrap();
        let hero = Combo::parse("AhQh").unwrap();
        let villain = Range::parse("KQs,KJs,QQ,JJ").unwrap();
        let cards = hotness(hero, &villain, &board, CardSet::EMPTY).unwrap();

        // Every card that can still come, minus the board and the hand.
        assert_eq!(
            cards.len(),
            47,
            "fifty-two cards minus the flop and the hand"
        );
        let at = |name: &str| {
            cards
                .iter()
                .find(|card| card.card == name)
                .expect("every live card appears")
                .equity
        };
        // A heart completes the nut flush; a blank does not.
        assert!(at("3h") > at("3c"), "a heart should be the best card");
        assert!(at("3h") > 0.9, "the nut flush is close to a lock");
        // A queen pairs up and helps too.
        assert!(at("Qc") > at("3c"));

        // No card left to come means no hotness.
        let river = Board::parse("Kh 7h 2c 3d 4s").unwrap();
        assert!(hotness(hero, &villain, &river, CardSet::EMPTY).is_none());
        assert!(hotness(hero, &villain, &Board::empty(), CardSet::EMPTY).is_none());
    }

    #[test]
    fn empty_ranges_return_nothing() {
        let board = Board::parse("Kc 7d 2s").unwrap();
        assert!(range_vs_range(
            &Range::empty(),
            &Range::parse("AA").unwrap(),
            &board,
            CardSet::EMPTY
        )
        .is_none());
    }
}
