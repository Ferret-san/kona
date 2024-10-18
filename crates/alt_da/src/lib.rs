#![doc = include_str!("../README.md")]
#![warn(missing_debug_implementations, missing_docs, unreachable_pub, rustdoc::all)]
#![deny(unused_must_use, rust_2018_idioms)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![no_std]

extern crate alloc;

pub mod alt_da;
pub mod source;
pub mod state;
pub mod traits;
pub mod types;

#[cfg(feature = "online")]
mod provider;
#[cfg(feature = "online")]
pub use provider::AltDAProvider;
#[cfg(feature = "online")]
mod da_client;
#[cfg(feature = "online")]
pub use da_client::DAClient;

#[cfg(test)]
pub mod test_utils;
