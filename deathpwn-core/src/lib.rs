#![forbid(unsafe_code)]

//! deathpwn-core: natural-language offensive-security terminal (library crate).
//! All logic and traits live here; the crate has no terminal or async-main deps.

pub mod config;
pub mod error;
pub mod schema;

pub use config::{Config, ProviderConfig};
pub use error::{DeathpwnError, Result};
