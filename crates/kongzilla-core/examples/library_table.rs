//! Prints the preflop library as a table, for checking the charts against the
//! solutions they were read from. Run with `make library`.

use kongzilla_core::library::{charts, Spot};

fn main() {
    println!(
        "{:<24} {:>8} {:>9} {:>8}   what it is",
        "chart", "size", "combos", "percent"
    );
    for chart in charts() {
        let size = if chart.spot == Spot::Defend {
            format!("vs {:.2}", chart.size_bb)
        } else {
            format!("{:.2}", chart.size_bb)
        };
        println!(
            "{:<24} {:>8} {:>9} {:>7.2}%   {}",
            chart.id,
            size.trim_end_matches('0').trim_end_matches('.'),
            format!("{:.1}", chart.combos()),
            chart.percent(),
            chart.description()
        );
    }
}
