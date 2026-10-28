//! Static files of the page, built into the binary: nothing is loaded from
//! elsewhere.

/// One built-in file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asset {
    /// Path under `/assets/` (`index.html` is also served at `/`).
    pub path: &'static str,
    pub mime: &'static str,
    pub bytes: &'static [u8],
}

macro_rules! asset {
    ($path:literal, $mime:expr) => {
        Asset {
            path: $path,
            mime: $mime,
            bytes: include_bytes!(concat!("assets/", $path)),
        }
    };
}

const HTML: &str = "text/html; charset=utf-8";
const JS: &str = "text/javascript; charset=utf-8";
const CSS: &str = "text/css; charset=utf-8";

const ALL: &[Asset] = &[
    asset!("index.html", HTML),
    asset!("app.js", JS),
    asset!("view.js", JS),
    asset!("app.css", CSS),
];

impl Asset {
    pub fn get(path: &str) -> Option<Asset> {
        ALL.iter().find(|a| a.path == path).copied()
    }

    pub fn all() -> &'static [Asset] {
        ALL
    }
}
