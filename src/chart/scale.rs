//! Backward compatibility shim for legacy scale module references.
//! The canonical implementation has moved to `crate::chart::ticks`.

#![allow(unused_imports)]

pub use super::ticks::*;

pub mod linear {
    pub use crate::chart::ticks::y::*;
}

pub mod timeseries {
    pub use crate::chart::ticks::x::*;
}
