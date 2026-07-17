#![forbid(unsafe_code)]

//! deathpwn-core: natural-language offensive-security terminal (library crate).
//! All logic and traits live here; the crate has no terminal or async-main deps.

pub mod cache;
pub mod cancel;
pub mod clock;
pub mod config;
pub mod detector;
pub mod error;
pub mod exec;

pub use exec::{CommandRunner, CommandSpec, OutputLine, RunOutcome, ShellRunner, Stream};
pub mod engine;
pub mod goal;
pub mod pipeline;
pub mod providers;
pub mod schema;
pub mod search;
pub mod session;

pub use cancel::CancelToken;
pub use config::{Config, ProviderConfig};
pub use error::{DeathpwnError, Result};
pub use search::{SearchProvider, SearchResult};
