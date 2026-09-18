//! Range and board analysis for Texas Hold'em.
//!
//! The crate is laid out as a pipeline:
//!
//! 1. [`cards`] gives every two-card combination a dense index in `0..1326`.
//! 2. [`range`] is a weight per combo, read and written in the usual text notation.
//! 3. [`stats`] classifies every combo against a board into a bitset of statistics,
//!    once per board. That cache is the only place the [`eval`]uator is used on the
//!    hot path.
//! 4. [`breakdown`], [`groups`] and [`equity`] are all reads over that cache.
//! 5. [`engine`] ties them together into the object an interface drives.
//!
//! ```
//! use kongzilla_core::prelude::*;
//!
//! let mut session = Session::new();
//! session.set_board_text("Kc Qh Jh").unwrap();
//! session.set_active_range_text("22+, A2s+, KJs+, AJo+").unwrap();
//!
//! let panel = session.breakdown();
//! let top_pair = panel.row(StatId::TOP_PAIR).unwrap();
//! assert!(top_pair.fraction > 0.0);
//!
//! // Paint the hands that continue, then press the flop's filter and read what
//! // is left.
//! session.paint_stat(StatId::TOP_PAIR, 1);
//! session.paint_stat(StatId::FLUSH_DRAW, 1);
//! session.toggle_street_filter(0);
//! println!("continues with {}", session.effective_range().to_notation());
//! ```

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod board;
pub mod breakdown;
pub mod cards;
pub mod engine;
pub mod equity;
pub mod error;
pub mod eval;
pub mod flops;
pub mod groups;
pub mod library;
mod library_charts;
pub mod notation;
pub mod preflop;
pub mod range;
pub mod ranking;
mod ranking_table;
pub mod rng;
pub mod stats;

/// The types an interface normally needs.
pub mod prelude {
    pub use crate::board::{Board, Street};
    pub use crate::breakdown::{Breakdown, BreakdownMode, StatRow};
    pub use crate::cards::{Card, CardSet, Combo, HandClass};
    pub use crate::engine::{Cut, Player, Session, Snapshot};
    pub use crate::equity::{Equity, EquityReport};
    pub use crate::error::ParseError;
    pub use crate::groups::{Colour, GroupSet, Mark};
    pub use crate::library::{Chart, Seat, Spot, Stack};
    pub use crate::range::Range;
    pub use crate::ranking::Ranking;
    pub use crate::stats::{ClassifyOptions, StatBlock, StatId, StatMask};
}
