use thiserror::Error;

/// Top-level error for deathpwn-core. Failover and the feedback loop absorb the
/// *expected* failures; this type is for the rest.
#[derive(Debug, Error)]
pub enum DeathpwnError {
    #[error("config error: {0}")]
    Config(String),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("search error: {0}")]
    Search(String),

    #[error("exec error: {0}")]
    Exec(String),

    #[error("schema error: {0}")]
    Schema(String),

    #[error("cache error: {0}")]
    Cache(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("operation cancelled")]
    Cancelled,
}

/// Crate-wide result alias.
pub type Result<T> = std::result::Result<T, DeathpwnError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_converts_via_from() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "nope");
        let err: DeathpwnError = io.into();
        match err {
            DeathpwnError::Io(inner) => {
                assert_eq!(inner.kind(), std::io::ErrorKind::NotFound);
            }
            other => panic!("expected Io variant, got {other:?}"),
        }
    }

    #[test]
    fn config_display_includes_message() {
        let err = DeathpwnError::Config("missing DEATHPWN_PROVIDER_A_URL".to_string());
        assert!(err.to_string().contains("DEATHPWN_PROVIDER_A_URL"));
    }

    #[test]
    fn result_alias_carries_deathpwn_error() {
        fn ok() -> Result<u32> {
            Ok(7)
        }
        fn cancelled() -> Result<u32> {
            Err(DeathpwnError::Cancelled)
        }
        assert_eq!(ok().unwrap(), 7);
        assert!(matches!(cancelled(), Err(DeathpwnError::Cancelled)));
    }
}
