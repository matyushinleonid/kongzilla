//! Times the preflop pass over all 22,100 flops, which is the heaviest thing the
//! engine does. Run with `make bench`.

use kongzilla_core::cards::CardSet;
use kongzilla_core::preflop::over_all_flops;
use kongzilla_core::range::Range;
use kongzilla_core::stats::{ClassifyOptions, StatMask};
use std::time::Instant;

fn main() {
    for (label, notation) in [
        ("100% (everything)", "22+, A2+, K2+, Q2+, J2+, T2+, 92+, 82+, 72+, 62+, 52+, 42+, 32+"),
        ("51% (40bb button)", "22+, A2s+, K2s+, Q2s+, J5s+, T5s+, 95s+, 84s+, 74s+, 63s+, 53s+, 43s, A2o+, K7o+, Q8o+, J8o+, T8o+, 98o, 87o, 76o"),
        ("18% (100bb middle)", "55+, A7s+, A5s-A2s, K9s+, Q9s+, J9s+, T8s+, 98s, 87s, ATo+, KJo+, QJo"),
        ("1 combo", "AhKh"),
    ] {
        let range = Range::parse(notation).expect("the benchmark ranges parse");
        let started = Instant::now();
        let result = over_all_flops(
            &range,
            CardSet::EMPTY,
            ClassifyOptions::default(),
            StatMask::EMPTY,
        );
        let elapsed = started.elapsed();
        println!(
            "{label:<20} {:>6.0} combos  {:>12.0} classifications  {:>8.0?}",
            range.combo_count(),
            result.total,
            elapsed
        );
    }
}
