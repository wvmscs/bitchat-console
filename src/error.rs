//! Error type of the crate.

/// Errors returned by this crate. Variants come with the lots that need
/// them (see `PLAN_ET1.md`).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// No Tor: the console never connects to a relay in clear (D1).
    #[error("Tor is not reachable at {0}: start tor, or give its SOCKS address with --tor-socks")]
    TorUnavailable(String),
    /// Arti could not start or bootstrap.
    #[error("Tor (Arti): {0}")]
    Tor(String),
    #[error("invalid geohash {0:?}")]
    InvalidGeohash(String),
    /// BitChat channels have 2, 4, 5, 6 or 7 characters.
    #[error("{0:?} is not a channel: BitChat channels have 2, 4, 5, 6 or 7 characters")]
    NotAChannel(String),
    #[error("the console has stopped")]
    ConsoleStopped,
    #[error("no area followed yet")]
    NoArea,
    #[error("{0:?} is not one of the followed cells")]
    NotInArea(String),
    #[error("empty message")]
    EmptyMessage,
    /// The app carries at most 255 bytes of text in a private message.
    #[error("private message too long: the app takes at most 255 bytes")]
    DmTooLong,
    #[error("{0:?} is not a public key (64 hex digits)")]
    InvalidKey(String),
    /// A key file other users can read: the console refuses to use it.
    #[error("{0} can be read by other users: run `chmod 600` on it")]
    InsecureKeyFile(String),
    #[error("{0} is not a key file of this console (wrong size)")]
    BadKeyFile(String),
    #[error("no {platform} relay list: {reason}, and no cached copy")]
    NoRelayList { platform: String, reason: String },
    #[error("HTTP: {0}")]
    Http(String),
    #[error("TLS: {0}")]
    Tls(String),
    /// A non-loopback address without `--allow-remote`.
    #[error("listening on {0} (not a loopback address) needs --allow-remote")]
    RemoteListen(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
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
