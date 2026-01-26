//! Target-specific implementations.

mod chip;
pub mod ws63;

pub use chip::{ChipConfig, ChipFamily, ChipOps};
