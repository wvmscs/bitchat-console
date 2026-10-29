// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! `bitchat-console`: follow a BitChat location channel and its 8
//! neighbours through Tor, write, and talk in private, in the browser.
//!
//!   bitchat-console <geohash> [--teleport] [--data-dir ~/.bitchat-console]
//!       [--tor-socks 127.0.0.1:9050 | --arti] [--relays-csv <url|path>]
//!       [--android-csv <url|path> | --no-android] [--apps both|ios|android] [--strategy AS]
//!       [--dm-relays <url,url>] [--no-history]
//!       [--no-presence] [--pow-bits 8] [--listen 127.0.0.1:8090] [--allow-remote]
//!       [--tls-cert <pem> --tls-key <pem>] [--map http://127.0.0.1:8080] [--open] [--verbose]
//!
//! Every message is kept in `<data-dir>/history.sqlite` (D31), unless
//! `--no-history`.
//!
//! With `--map`, a running bitchat-geohash map is served at `/` with the
//! console floating over it (lot 9).
//!
//! Exit code 2 for a wrong command line, 1 when the console cannot start.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use bitchat_console::connector::{Connector, Socks5};
use bitchat_console::console::{Console, ConsoleConfig};
use bitchat_console::identity::Keys;
use bitchat_console::lists::{ListSource, load};
use bitchat_console::server::{ServerConfig, TlsFiles, serve};
use geohash_relays::{Apps, Platform};

const USAGE: &str = "usage: bitchat-console <geohash> [--teleport] [--data-dir <dir>] \
     [--tor-socks 127.0.0.1:9050 | --arti] [--relays-csv <url|path>] \
     [--android-csv <url|path> | --no-android] [--apps both|ios|android] [--strategy AS] \
     [--dm-relays <url,url>] [--no-history] [--no-presence] \
     [--pow-bits 8] [--listen 127.0.0.1:8090] [--allow-remote] \
     [--tls-cert <pem> --tls-key <pem>] [--map http://127.0.0.1:8080] [--open] [--verbose]";

/// Which Tor (D1, D24).
#[derive(Debug, Clone, PartialEq)]
enum Tor {
    Socks(SocketAddr),
    Arti,
}

#[derive(Debug)]
struct Args {
    geohash: String,
    teleport: bool,
    data_dir: PathBuf,
    tor: Tor,
    ios: ListSource,
    android: Option<ListSource>,
    apps: Apps,
    /// The relay strategy (`AS` by default).
    strategy: bitchat_console::strategies::Name,
    /// Relays of private messages; `None`: the apps' defaults.
    dm_relays: Option<Vec<String>>,
    presence: bool,
    /// Keep every message in `history.sqlite` (D31).
    history: bool,
    pow_bits: u32,
    server: ServerConfig,
    open: bool,
    verbose: bool,
}

fn default_data_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".bitchat-console")
}

/// Tor in the binary when it is built with Arti, else the Tor daemon (D24).
fn default_tor() -> Tor {
    if cfg!(feature = "arti") {
        Tor::Arti
    } else {
        Tor::Socks(SocketAddr::from(([127, 0, 0, 1], 9050)))
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut it = args.into_iter();
    let mut a = Args {
        geohash: String::new(),
        teleport: false,
        data_dir: default_data_dir(),
        tor: default_tor(),
        ios: ListSource::official(Platform::Ios),
        android: Some(ListSource::official(Platform::Android)),
        apps: Apps::Both,
        strategy: bitchat_console::strategies::Name::As,
        dm_relays: None,
        presence: true,
        history: true,
        pow_bits: bitchat_console::pow::TARGET_BITS,
        server: ServerConfig::default(),
        open: false,
        verbose: false,
    };
    let (mut cert, mut key) = (None::<PathBuf>, None::<PathBuf>);
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--teleport" => a.teleport = true,
            "--data-dir" => a.data_dir = value()?.into(),
            "--tor-socks" => {
                let v = value()?;
                a.tor = Tor::Socks(
                    v.parse()
                        .map_err(|_| format!("--tor-socks: address:port expected, got {v:?}"))?,
                )
            }
            "--arti" => {
                if !cfg!(feature = "arti") {
                    return Err(
                        "--arti: this binary is built without Arti (cargo build --features arti)"
                            .into(),
                    );
                }
                a.tor = Tor::Arti
            }
            "--relays-csv" => a.ios = ListSource::parse(&value()?),
            "--android-csv" => a.android = Some(ListSource::parse(&value()?)),
            "--no-android" => a.android = None,
            "--strategy" => {
                let v = value()?;
                a.strategy = bitchat_console::strategies::Name::parse(&v).ok_or_else(|| {
                    format!(
                        "--strategy: one of {}, got {v:?}",
                        bitchat_console::strategies::Name::ALL
                    )
                })?;
            }
            "--apps" => {
                a.apps = match value()?.as_str() {
                    "both" => Apps::Both,
                    "ios" => Apps::Ios,
                    "android" => Apps::Android,
                    v => return Err(format!("--apps: both, ios or android, got {v:?}")),
                }
            }
            "--dm-relays" => {
                a.dm_relays = Some(
                    value()?
                        .split(',')
                        .map(|r| r.trim().to_owned())
                        .filter(|r| !r.is_empty())
                        .collect(),
                )
            }
            "--no-presence" => a.presence = false,
            "--no-history" => a.history = false,
            "--pow-bits" => {
                let v = value()?;
                a.pow_bits = v
                    .parse()
                    .ok()
                    .filter(|b| *b <= 32)
                    .ok_or(format!("--pow-bits: 0 to 32, got {v:?}"))?
            }
            "--listen" => {
                let v = value()?;
                a.server.listen = v
                    .parse()
                    .map_err(|_| format!("--listen: address:port expected, got {v:?}"))?
            }
            "--allow-remote" => a.server.allow_remote = true,
            "--map" => a.server.map = Some(value()?),
            "--tls-cert" => cert = Some(value()?.into()),
            "--tls-key" => key = Some(value()?.into()),
            "--open" => a.open = true,
            "--verbose" => a.verbose = true,
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            _ if a.geohash.is_empty() => a.geohash = arg,
            _ => return Err(format!("one geohash only, got {arg:?} too")),
        }
    }
    if a.geohash.is_empty() {
        return Err("the geohash to follow is missing".into());
    }
    a.server.tls = match (cert, key) {
        (Some(cert), Some(key)) => Some(TlsFiles { cert, key }),
        (None, None) => None,
        _ => return Err("--tls-cert and --tls-key go together".into()),
    };
    Ok(a)
}

fn fail(what: &str, e: impl std::fmt::Display) -> ! {
    eprintln!("bitchat-console: {what}: {e}");
    std::process::exit(1)
}

async fn connector(tor: &Tor, data_dir: &std::path::Path) -> Arc<dyn Connector> {
    match tor {
        Tor::Socks(addr) => {
            let s = Socks5::new(*addr);
            if let Err(e) = s.check().await {
                fail("no Tor", e);
            }
            Arc::new(s)
        }
        #[cfg(feature = "arti")]
        Tor::Arti => {
            eprintln!("starting Tor (Arti)…");
            match bitchat_console::arti::Arti::bootstrap(data_dir).await {
                Ok(a) => Arc::new(a),
                Err(e) => fail("no Tor", e),
            }
        }
        #[cfg(not(feature = "arti"))]
        Tor::Arti => {
            let _ = data_dir;
            fail("no Tor", "built without Arti")
        }
    }
}

fn open_browser(url: &str) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(program).arg(url).spawn() {
        tracing::warn!(error = %e, "cannot open the browser ({program})");
    }
}

#[tokio::main]
async fn main() {
    let args = parse_args(std::env::args().skip(1)).unwrap_or_else(|e| {
        eprintln!("bitchat-console: {e}\n{USAGE}");
        std::process::exit(2)
    });
    tracing_subscriber::fmt()
        .with_max_level(if args.verbose {
            tracing::Level::DEBUG
        } else {
            tracing::Level::INFO
        })
        .with_writer(std::io::stderr)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .init();
    bitchat_console::install_crypto_provider();

    // A busy port is found now, not after Tor and the lists.
    if args.server.listen.port() != 0
        && let Err(e) = std::net::TcpListener::bind(args.server.listen)
        && e.kind() == std::io::ErrorKind::AddrInUse
    {
        fail(
            "cannot serve the page",
            format!(
                "{} is already in use: choose another port with --listen",
                args.server.listen
            ),
        );
    }
    let keys = Keys::load_or_create(&args.data_dir)
        .unwrap_or_else(|e| fail(&format!("keys in {}", args.data_dir.display()), e));
    let tor = connector(&args.tor, &args.data_dir).await;
    eprintln!("Tor: {}", tor.describe());
    let cache = args.data_dir.join("lists");
    let ios = load(&args.ios, Platform::Ios, tor.clone(), &cache)
        .await
        .unwrap_or_else(|e| fail("relay list", e));
    let android = match &args.android {
        Some(src) => Some(
            load(src, Platform::Android, tor.clone(), &cache)
                .await
                .unwrap_or_else(|e| fail("relay list", e)),
        ),
        None => None,
    };
    let inputs = bitchat_console::strategies::Inputs {
        ios,
        android,
        apps: args.apps,
    };
    let strategy = bitchat_console::strategies::build(args.strategy, &inputs);
    let mut config = ConsoleConfig::new(strategy, tor, Arc::new(keys.seed));
    config.noise = Some(Arc::new(keys.noise));
    config.presence = args.presence;
    let db_path = args.data_dir.join(bitchat_console::db::DB_FILE);
    if let Err(e) = bitchat_console::db::Db::open(&db_path) {
        fail(&format!("local storage {}", db_path.display()), e);
    }
    config.db_path = Some(db_path);
    if args.history {
        let path = args.data_dir.join(bitchat_console::history::HISTORY_FILE);
        if let Err(e) = bitchat_console::history::History::open(&path) {
            fail(&format!("history {}", path.display()), e);
        }
        config.history_path = Some(path);
    }
    if let Some(dm) = args.dm_relays.clone() {
        config.dm_relays = dm;
    }
    config.pow_bits = args.pow_bits;
    let console = Arc::new(Console::start(config));
    let cells = console
        .set_area(&args.geohash, args.teleport)
        .await
        .unwrap_or_else(|e| fail("cannot follow", e));
    eprintln!(
        "following #{} and {} neighbours{}",
        cells[0],
        cells.len() - 1,
        if args.teleport { " (teleported)" } else { "" }
    );
    let server = serve(console.clone(), args.server.clone())
        .await
        .unwrap_or_else(|e| fail("cannot serve the page", e));
    let url = format!("{}/", server.url());
    eprintln!("console: {url}");
    if args.open {
        open_browser(&url);
    }
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("stopping");
    server.shutdown().await;
    if let Ok(c) = Arc::try_unwrap(console) {
        c.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Args, String> {
        parse_args(s.split_whitespace().map(str::to_owned))
    }

    #[test]
    fn options() {
        let a = parse(
            "u09tv --teleport --data-dir /tmp/x --tor-socks 127.0.0.1:9150 --relays-csv own.csv \
             --no-android --apps ios --no-presence --pow-bits 12 --listen 127.0.0.1:9000 --open \
             --dm-relays wss://a,wss://b --no-history",
        )
        .unwrap();
        assert_eq!(a.geohash, "u09tv");
        assert!(a.teleport && !a.presence && a.open && !a.history);
        assert!(parse("u09tv").unwrap().history);
        assert_eq!(a.tor, Tor::Socks("127.0.0.1:9150".parse().unwrap()));
        assert_eq!(a.ios, ListSource::Path("own.csv".into()));
        assert!(a.android.is_none());
        assert_eq!((a.apps, a.pow_bits), (Apps::Ios, 12));
        assert_eq!(a.server.listen, "127.0.0.1:9000".parse().unwrap());
        assert_eq!(a.dm_relays, Some(vec!["wss://a".into(), "wss://b".into()]));
    }

    #[test]
    fn errors() {
        let err = |s: &str| parse(s).err().unwrap();
        assert_eq!(err("--teleport"), "the geohash to follow is missing");
        assert_eq!(err("u09tv --nope"), "unknown option --nope");
        assert!(err("u09tv --apps web").contains("both, ios or android"));
        assert!(err("u09tv --tls-cert c.pem").contains("go together"));
        assert!(err("u09tv --pow-bits 99").contains("0 to 32"));
        assert!(err("u09tv --tor-socks localhost").contains("address:port"));
    }
}
