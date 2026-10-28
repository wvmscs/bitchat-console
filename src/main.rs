//! `bitchat-console`: lot 0 skeleton; the command line comes with the next
//! lots (see `PLAN_ET1.md`).

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    eprintln!(
        "bitchat-console {}: not usable yet (see PLAN_ET1.md)",
        env!("CARGO_PKG_VERSION")
    );
    std::process::exit(2)
}
