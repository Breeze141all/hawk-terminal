//! Backward compatibility shim for legacy scale module references.
//! The canonical implementation has moved to `crate::chart::ticks`.

pub use super::ticks::*;

pub mod linear {
    pub use crate::chart::ticks::y::*;
}
