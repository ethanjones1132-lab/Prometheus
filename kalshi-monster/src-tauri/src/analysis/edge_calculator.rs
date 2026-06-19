//! Edge Calculator — re-export of the shared `monster-edge-core` crate.
//!
//! The math here was byte-for-byte identical to prizepicks-monster's
//! edge_calculator, so both apps now share one pure crate (and one
//! embedded calibrator artifact). Behavior is unchanged; all existing
//! `crate::analysis::edge_calculator::*` paths keep working.

pub use monster_edge_core::*;
