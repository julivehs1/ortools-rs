//! Rust bindings to Google's CP-SAT constraint solver.
//!
//! ```no_run
//! use cpsat::CpModelBuilder;
//!
//! let mut m = CpModelBuilder::default();
//! let x = m.new_int_var(0..=10);
//! let y = m.new_int_var(0..=10);
//! m.add_le(x + y, 12);
//! m.maximize(x * 2 + y);
//!
//! let response = m.solve();
//! println!("x = {}", x.value(&response));
//! ```
//!
//! # Scope
//!
//! CP-SAT's model is a protobuf. Everything here is a convenience layer over
//! that proto — which means [`CpModelBuilder::proto_mut`] is always available
//! as an escape hatch. If this crate has not wrapped a constraint yet, reach
//! through and set it directly; nothing is hidden from you.

mod builder;
mod ffi;

pub use builder::{BoolVar, Constraint, CpModelBuilder, IntVar, IntervalVar, LinearExpr};

/// Generated protobuf types for the CP-SAT model and solver parameters.
///
/// Lints are relaxed here: the doc comments are carried over verbatim from
/// OR-Tools' `.proto` files and are not ours to reformat.
#[allow(clippy::doc_overindented_list_items, clippy::doc_lazy_continuation)]
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/operations_research.sat.rs"));
}

pub use ffi::{model_stats, solve, solve_with_parameters, validate};

/// Outcome of a solve, mirroring `CpSolverStatus`.
pub use proto::CpSolverStatus as Status;
