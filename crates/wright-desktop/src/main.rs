//! `wright-desktop`: Wright's native app. Same as `wright` with no
//! arguments, or `wright app`.
//!
//!     wright-desktop                                   # start a run
//!     wright-desktop --replay demo/aircraft/events.jsonl

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    wright_desktop::launch(wright_desktop::Options::from_args(&args)?)
}
