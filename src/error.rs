//! Error type of the crate.

/// Errors returned by this crate. Variants come with the lots that need
/// them (see `PLAN_ET1.md`).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// No Tor: the console never connects to a relay in clear (D1).
    #[error("Tor is not reachable at {0}: start tor, or give its SOCKS address with --tor-socks")]
    TorUnavailable(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tor_error_says_what_to_do() {
        let e = Error::TorUnavailable("127.0.0.1:9050".into()).to_string();
        assert!(e.contains("--tor-socks"), "{e}");
    }
}
