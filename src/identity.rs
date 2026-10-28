//! Keys of the console (lot 3).
//!
//! * One Nostr key per geohash cell, derived from a device seed exactly as
//!   the app does, so that keys of two cells cannot be linked
//!   (`NostrIdentityBridge.deriveIdentity(forGeohash:)`).
//! * A Noise identity (X25519 static key and Ed25519 signing key), as a
//!   BitChat peer has, for the mixed groups (lot 10).
//!
//! Both live in the data directory, in files only the user can read; they
//! never leave the machine and are never logged.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use hmac::{Hmac, Mac};
use k256::schnorr::SigningKey;
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// The device seed: 32 random bytes, the root of every cell key.
pub struct Seed([u8; 32]);

impl Seed {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Seed(bytes)
    }

    /// The Nostr key of a cell: `HMAC-SHA256(seed, geohash ‖ i)` with `i` a
    /// big-endian `u32` from 0, the first that is a valid secp256k1 secret
    /// (the app tries 10).
    pub fn cell(&self, geohash: &str) -> Result<CellKey> {
        if geohash.is_empty() || geohash_relays::geohash::bbox(geohash).is_err() {
            return Err(Error::InvalidGeohash(geohash.to_owned()));
        }
        for i in 0u32..10 {
            let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.0).expect("any key size");
            mac.update(geohash.as_bytes());
            mac.update(&i.to_be_bytes());
            let candidate = mac.finalize().into_bytes();
            if let Ok(key) = SigningKey::from_bytes(&candidate) {
                return Ok(CellKey {
                    geohash: geohash.to_owned(),
                    key,
                });
            }
        }
        // Probability about 10 × 2⁻¹²⁸.
        Err(Error::InvalidGeohash(geohash.to_owned()))
    }

    /// A key for another use than a cell, unlinkable to the cell keys: the
    /// same derivation with a label that is not a geohash (the app does
    /// this for its mesh bridge, `"bridge|" + cell`). Groups use
    /// `"group|" + group ID` (D27).
    pub fn labelled(&self, label: &str) -> Result<CellKey> {
        for i in 0u32..10 {
            let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.0).expect("any key size");
            mac.update(label.as_bytes());
            mac.update(&i.to_be_bytes());
            if let Ok(key) = SigningKey::from_bytes(&mac.finalize().into_bytes()) {
                return Ok(CellKey {
                    geohash: label.to_owned(),
                    key,
                });
            }
        }
        Err(Error::InvalidGeohash(label.to_owned()))
    }
}

/// The key the console signs with in one cell.
#[derive(Clone)]
pub struct CellKey {
    geohash: String,
    key: SigningKey,
}

impl CellKey {
    pub fn geohash(&self) -> &str {
        &self.geohash
    }

    /// The x-only public key, hex: the event `pubkey`.
    pub fn pubkey_hex(&self) -> String {
        hex(&self.key.verifying_key().to_bytes())
    }

    /// The last 4 hex digits, as the app shows after a nickname (`name#abcd`).
    pub fn suffix(&self) -> String {
        let p = self.pubkey_hex();
        p[p.len() - 4..].to_owned()
    }

    pub fn signing_key(&self) -> &SigningKey {
        &self.key
    }
}

impl std::fmt::Debug for CellKey {
    // Never print the secret.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CellKey({}, {})", self.geohash, self.pubkey_hex())
    }
}

/// The console as a BitChat peer: Noise static key (X25519) and signing key
/// (Ed25519).
pub struct NoiseIdentity {
    static_key: x25519_dalek::StaticSecret,
    signing: ed25519_dalek::SigningKey,
}

impl NoiseIdentity {
    pub fn from_bytes(static_key: [u8; 32], signing: [u8; 32]) -> Self {
        NoiseIdentity {
            static_key: x25519_dalek::StaticSecret::from(static_key),
            signing: ed25519_dalek::SigningKey::from_bytes(&signing),
        }
    }

    pub fn static_public(&self) -> [u8; 32] {
        x25519_dalek::PublicKey::from(&self.static_key).to_bytes()
    }

    pub fn signing_public(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    /// SHA-256 of the static public key, hex (`getIdentityFingerprint`).
    pub fn fingerprint(&self) -> String {
        hex(&Sha256::digest(self.static_public()))
    }

    /// The 16-hex peer ID: the start of the fingerprint
    /// (`PeerID(publicKey:)`).
    pub fn peer_id(&self) -> String {
        self.fingerprint()[..16].to_owned()
    }

    pub fn signing_key(&self) -> &ed25519_dalek::SigningKey {
        &self.signing
    }

    pub fn static_secret(&self) -> &x25519_dalek::StaticSecret {
        &self.static_key
    }
}

/// The console's keys, loaded from (or created in) the data directory.
pub struct Keys {
    pub seed: Seed,
    pub noise: NoiseIdentity,
}

/// File of the seed, in the data directory.
pub const SEED_FILE: &str = "seed.key";
/// File of the Noise identity (static key, then signing key).
pub const NOISE_FILE: &str = "noise.key";

impl Keys {
    /// Loads the keys, or creates them on first use. The directory is
    /// created `0700`, the files `0600`; a key file others can read is
    /// refused.
    pub fn load_or_create(dir: &Path) -> Result<Keys> {
        create_private_dir(dir)?;
        let seed: [u8; 32] = load_or_create(&dir.join(SEED_FILE), 32)?
            .try_into()
            .expect("checked size");
        let noise = load_or_create(&dir.join(NOISE_FILE), 64)?;
        let (s, k) = noise.split_at(32);
        Ok(Keys {
            seed: Seed(seed),
            noise: NoiseIdentity::from_bytes(
                s.try_into().expect("32 bytes"),
                k.try_into().expect("32 bytes"),
            ),
        })
    }
}

fn create_private_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn load_or_create(path: &PathBuf, len: usize) -> Result<Vec<u8>> {
    let name = path.display().to_string();
    if path.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if fs::metadata(path)?.permissions().mode() & 0o077 != 0 {
                return Err(Error::InsecureKeyFile(name));
            }
        }
        let bytes = fs::read(path)?;
        if bytes.len() != len {
            return Err(Error::BadKeyFile(name));
        }
        return Ok(bytes);
    }
    let mut bytes = vec![0u8; len];
    OsRng.fill_bytes(&mut bytes);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(path)?;
    f.write_all(&bytes)?;
    f.sync_all()?;
    tracing::info!(file = %name, "new key created");
    Ok(bytes)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
