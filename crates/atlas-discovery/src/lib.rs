//! Conservative alignment. Source assertions, identity mappings and asset edges are distinct.
//! Outputs stage an ingest bundle; this crate never mutates the atlas identity layer.

pub mod align;
mod build;
pub mod candidates;
pub mod evaluation;
mod export;
mod model;

pub use build::{build, load};
pub use export::*;
pub use model::*;
