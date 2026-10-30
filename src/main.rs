// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! `bitchat-console`: follow a BitChat location channel and its 8
//! neighbours through Tor, write, and talk in private, in the browser.
//!
//!   bitchat-console <geohash> [--teleport] [--data-dir ~/.bitchat-console]
//!       [--tor-socks 127.0.0.1:9050 | --arti] [--relays-csv <url|path>]
//!       [--android-csv <url|path> | --no-android] [--apps both|ios|android] [--strategy AS|A0|A1|A5] [--shared-relays 0|1|2] [--hrw-salt <hex>] [--monitors <keys> --monitor-relays <urls>] [--republish] [--announce-relays] [--federation <urls> [--federation-check]] [--qualify | --qualify-probe]
//!       [--dm-relays <url,url>] [--no-history]
//!       [--no-presence] [--pow-bits 8] [--listen 127.0.0.1:8090] [--allow-remote]
//!       [--tls-cert <pem> --tls-key <pem> | --behind-proxy https://name
//!        [--proxy-from 127.0.0.1/8,::1] [--no-proxy-check]]
//!       [--map http://127.0.0.1:8080] [--open] [--verbose] [--geoip-db <file.mmdb>]
//!
//! With the `geocode` feature (default), `<geohash>` may also be `#u09tv`, a
//! place or an IP address (`"Rue de Rivoli, Paris"`, `8.8.8.8` with
//! `--geoip-db`, the DB-IP file downloaded there if missing): the finest
//! channel that holds it (lot 19, D35). Those lookups do not go through Tor.
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
use bitchat_console::server::{Net, ProxyConfig, ServerConfig, TlsFiles, serve};
use geohash_relays::{Apps, Platform};

const USAGE: &str = "usage: bitchat-console <geohash> [--teleport] [--data-dir <dir>] \
     [--tor-socks 127.0.0.1:9050 | --arti] [--relays-csv <url|path>] \
     [--android-csv <url|path> | --no-android] [--apps both|ios|android] [--strategy AS|A0|A1|A5] [--shared-relays 0|1|2] [--hrw-salt <hex>] [--monitors <keys> --monitor-relays <urls>] [--republish] [--announce-relays] [--federation <urls> [--federation-check]] [--qualify | --qualify-probe] \
     [--dm-relays <url,url>] [--no-history] [--no-presence] \
     [--pow-bits 8] [--listen 127.0.0.1:8090] [--allow-remote] \
     [--tls-cert <pem> --tls-key <pem> | --behind-proxy https://<name> \
     [--proxy-from <addr|net>,...] [--no-proxy-check]] \
     [--map http://127.0.0.1:8080] [--open] [--verbose] [--geoip-db <file.mmdb>]\n\
     <geohash>: also #geohash, a place or an IP address (built with the geocode feature)";

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
    /// Republish between the iOS and Android relay sets (A4).
    republish: bool,
    /// Announce the relays written to in presences, read the announced ones (A3).
    announce: bool,
    /// A0: shared relays added to every cell.
    shared_relays: usize,
    /// A1: the salt of the rendezvous hashing.
    salt: Option<Vec<u8>>,
    /// A2: trusted NIP-66 monitors, and where to read them.
    monitors: std::collections::BTreeSet<String>,
    monitor_relays: Vec<String>,
    /// A5: the members of the federation; check them and exit.
    federation: Vec<String>,
    federation_check: bool,
    /// Qualify the relays of both lists and exit (A6); `true`: also write
    /// test events.
    qualify: Option<bool>,
    /// The relay strategy (`AS` by default).
    strategy: bitchat_console::strategies::Name,
    /// Relays of private messages; `None`: the apps' defaults.
    dm_relays: Option<Vec<String>>,
    presence: bool,
    /// Keep every message in `history.sqlite` (D31).
    history: bool,
    pow_bits: u32,
    /// DB-IP file for IP addresses typed as cells (lot 19).
    #[cfg_attr(not(feature = "geocode"), allow(dead_code))]
    geoip_db: Option<PathBuf>,
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
        republish: false,
        announce: false,
        shared_relays: 0,
        salt: None,
        monitors: Default::default(),
        monitor_relays: Vec::new(),
        federation: Vec::new(),
        federation_check: false,
        qualify: None,
        dm_relays: None,
        presence: true,
        history: true,
        pow_bits: bitchat_console::pow::TARGET_BITS,
        geoip_db: None,
        server: ServerConfig::default(),
        open: false,
        verbose: false,
    };
    let (mut cert, mut key) = (None::<PathBuf>, None::<PathBuf>);
    let (mut public, mut from, mut no_check) = (None::<String>, None::<Vec<Net>>, false);
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
            "--republish" => a.republish = true,
            "--announce-relays" => a.announce = true,
            "--qualify" => a.qualify = Some(false),
            "--qualify-probe" => a.qualify = Some(true),
            "--monitors" => {
                a.monitors = value()?
                    .split(',')
                    .map(|k| k.trim().to_lowercase())
                    .filter(|k| !k.is_empty())
                    .collect();
                if a.monitors
                    .iter()
                    .any(|k| k.len() != 64 || bitchat_console::nostr::hex_decode(k).is_none())
                {
                    return Err("--monitors: public keys, 64 hex digits, comma-separated".into());
                }
            }
            "--federation" => {
                a.federation = value()?
                    .split(',')
                    .map(|r| r.trim().to_owned())
                    .filter(|r| !r.is_empty())
                    .collect();
            }
            "--federation-check" => a.federation_check = true,
            "--monitor-relays" => {
                a.monitor_relays = value()?
                    .split(',')
                    .map(|r| r.trim().to_owned())
                    .filter(|r| !r.is_empty())
                    .collect();
            }
            "--hrw-salt" => {
                let v = value()?;
                a.salt = Some(
                    bitchat_console::nostr::hex_decode(&v)
                        .filter(|b| !b.is_empty())
                        .ok_or_else(|| format!("--hrw-salt: hexadecimal bytes, got {v:?}"))?,
                );
            }
            "--shared-relays" => {
                a.shared_relays = match value()?.as_str() {
                    "0" => 0,
                    "1" => 1,
                    "2" => 2,
                    v => return Err(format!("--shared-relays: 0, 1 or 2, got {v:?}")),
                }
            }
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
            "--behind-proxy" => public = Some(value()?),
            "--proxy-from" => {
                let list = value()?;
                let nets = list
                    .split(',')
                    .map(|n| Net::parse(n.trim()).map_err(|e| format!("--proxy-from: {e}")))
                    .collect::<Result<Vec<_>, _>>()?;
                from.get_or_insert_with(Vec::new).extend(nets);
            }
            "--no-proxy-check" => no_check = true,
            "--open" => a.open = true,
            "--verbose" => a.verbose = true,
            "--geoip-db" => {
                if !cfg!(feature = "geocode") {
                    return Err(
                        "--geoip-db: this binary is built without the geocode feature".into(),
                    );
                }
                a.geoip_db = Some(value()?.into())
            }
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
    a.server.proxy = match public {
        Some(public) => {
            if a.server.tls.is_some() {
                return Err(
                    "--behind-proxy: the proxy does the TLS (no --tls-cert nor --tls-key)".into(),
                );
            }
            let mut p = ProxyConfig::new(&public).map_err(|e| format!("--behind-proxy: {e}"))?;
            if let Some(from) = from {
                p.from = from;
            }
            p.check = !no_check;
            Some(p)
        }
        None if from.is_some() || no_check => {
            return Err("--proxy-from and --no-proxy-check go with --behind-proxy".into());
        }
        None => None,
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
    #[allow(unused_mut)]
    let mut args = parse_args(std::env::args().skip(1)).unwrap_or_else(|e| {
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

    // The cell typed may be a place (lot 19, D35): its channel, before
    // anything else. Not through Tor: said in the README.
    #[cfg(feature = "geocode")]
    {
        use bitchat_console::geocode::Places;
        let places = Places::new(args.geoip_db.as_deref())
            .await
            .unwrap_or_else(|e| fail("cannot look places up", e));
        let r = places
            .resolve(&args.geohash)
            .await
            .unwrap_or_else(|e| fail("cannot find the cell", e));
        if let Some(p) = &r.place {
            eprintln!("cell: #{} for {} ({})", r.geohash, p.label, p.attribution);
        }
        args.geohash = r.geohash;
        args.server.places = Some(Arc::new(places));
    }

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
    if args.federation_check {
        if args.federation.is_empty() {
            fail("--federation-check", "give the members with --federation");
        }
        let cells = geohash_relays::geohash::with_neighbours(&args.geohash)
            .unwrap_or_else(|e| fail("--federation-check", e));
        let (omissions, failed) =
            bitchat_console::federation::check(&args.federation, &cells, tor.clone()).await;
        println!(
            "{}",
            bitchat_console::federation::report(&omissions, &failed)
        );
        return;
    }
    if let Some(probe) = args.qualify {
        let mut lists = vec![&ios];
        lists.extend(android.as_ref());
        let relays = bitchat_console::qualify::relays_of(&lists);
        eprintln!(
            "qualifying {} relays through Tor{}",
            relays.len(),
            if probe {
                ", writing one test event to each"
            } else {
                ""
            }
        );
        let (prober, complete) = bitchat_console::qualify::run(relays, tor.clone(), probe).await;
        let text = bitchat_console::qualify::report(&prober, probe, complete);
        let path = args.data_dir.join("qualification.md");
        if let Err(e) = std::fs::write(&path, &text) {
            eprintln!("cannot write {}: {e}", path.display());
        }
        println!("{text}");
        eprintln!("report: {}", path.display());
        return;
    }
    // A2: the trusted monitors' directory, if asked.
    let (mut ios, mut android) = (ios, android);
    let mut monitored = None;
    if !args.monitors.is_empty() {
        if args.monitor_relays.is_empty() {
            fail(
                "--monitors",
                "give the relays to read them from with --monitor-relays",
            );
        }
        match bitchat_console::monitors::load(
            args.monitors.clone(),
            args.monitor_relays.clone(),
            tor.clone(),
            &args.data_dir,
        )
        .await
        {
            Some((dir, list)) => {
                eprintln!("monitors: {} candidate relays", list.len());
                if !dir.qualification.verdicts.is_empty() {
                    let guard = |l: geohash_relays::RelayDirectory, name: &str| match dir
                        .guard(l.clone())
                    {
                        Ok((kept, true)) => kept,
                        _ => {
                            eprintln!(
                                "monitors would remove most of the {name} list: not used for it"
                            );
                            l
                        }
                    };
                    ios = guard(ios, "iOS");
                    android = android.map(|a| guard(a, "Android"));
                }
                monitored = Some(list);
            }
            None => eprintln!(
                "monitors: nothing read, no cached directory: the lists are used as they are"
            ),
        }
    }
    let inputs = bitchat_console::strategies::Inputs {
        ios,
        android,
        apps: args.apps,
        shared: args.shared_relays,
        salt: args.salt.clone(),
        monitored,
        federation: args.federation.clone(),
    };
    if args.strategy == bitchat_console::strategies::Name::A5 && args.federation.is_empty() {
        fail("--strategy A5", "give the members with --federation");
    }
    if args.strategy == bitchat_console::strategies::Name::A1 && args.salt.is_none() {
        eprintln!("warning: A1 without --hrw-salt: the relay choice can be ground in advance");
    }
    let (strategy, rebuild) = bitchat_console::strategies::build(args.strategy, &inputs);
    let connector = tor.clone();
    let mut config = ConsoleConfig::new(strategy, tor, Arc::new(keys.seed));
    config.rebuild = rebuild;
    config.announce = args.announce;
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
    if args.republish {
        eprintln!("republishing between the iOS and Android relays of the area (A4)");
        bitchat_console::republish::spawn(
            console.clone(),
            inputs.ios.clone(),
            inputs.android.clone(),
            connector,
        );
    }
    let server = serve(console.clone(), args.server.clone())
        .await
        .unwrap_or_else(|e| fail("cannot serve the page", e));
    let url = format!("{}/", server.url());
    eprintln!("console: {url}");
    // Behind a proxy the browser uses the public name: the local URL is
    // refused there (wrong host, not from the proxy).
    let url = match server.public_url() {
        Some(public) => {
            eprintln!("public: {public}/");
            format!("{public}/")
        }
        None => url,
    };
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
    fn strategy_options() {
        use bitchat_console::strategies::Name;
        let a = parse("u09tv --strategy a1 --hrw-salt 6b2f --republish --shared-relays 2").unwrap();
        assert_eq!(a.strategy, Name::A1);
        assert_eq!(a.salt, Some(vec![0x6b, 0x2f]));
        assert!(a.republish);
        assert_eq!(a.shared_relays, 2);
        assert_eq!(parse("u09tv --qualify").unwrap().qualify, Some(false));
        assert_eq!(parse("u09tv --qualify-probe").unwrap().qualify, Some(true));
        assert!(parse("u09tv --strategy A9").is_err());
        assert!(parse("u09tv --hrw-salt zz").is_err());
        assert!(parse("u09tv --shared-relays 3").is_err());
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

    #[test]
    fn behind_a_proxy() {
        let a = parse("u09tv --behind-proxy https://Console.Example.org").unwrap();
        let p = a.server.proxy.unwrap();
        assert_eq!(p.public, "https://console.example.org");
        assert_eq!(p.from, ProxyConfig::new("https://x").unwrap().from);
        assert!(p.check);
        let a = parse(
            "u09tv --behind-proxy https://c.example.org:8443 --listen 0.0.0.0:8090 \
             --allow-remote --proxy-from 172.17.0.0/16 --no-proxy-check",
        )
        .unwrap();
        let p = a.server.proxy.unwrap();
        assert_eq!(p.public, "https://c.example.org:8443");
        assert!(p.from[0].contains("172.17.0.5".parse().unwrap()));
        assert!(!p.check);
        let err = |s: &str| parse(s).err().unwrap();
        assert!(err("u09tv --behind-proxy http://c.example.org").contains("HTTPS"));
        assert!(
            err("u09tv --behind-proxy https://c --tls-cert c --tls-key k")
                .contains("the proxy does the TLS")
        );
        assert!(err("u09tv --behind-proxy https://c --proxy-from x").contains("--proxy-from"));
        assert!(err("u09tv --no-proxy-check").contains("go with --behind-proxy"));
    }
}
