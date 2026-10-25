//! CREAO AI (creao.ai) in Wright: the **Build Desk**, a CREAO agent that
//! takes a design whose checks pass and does the live-web half of building
//! it -- sources every BOM line at a current price and writes the builder's
//! packet (a build page) -- while Wright validates every price it returns.
//! See `docs/creao.md` for why, the Cal Hacks track, and how the agent is set up.
//!
//! * [`config`] -- the key (`capi_` API Trigger or `cr_sk_` Account) from `.env`, never printed
//! * [`client`] -- start / poll / download on both documented route families
//! * [`desk`]   -- Wright's side: the design packet, quote validation, events
//! * [`transport`] -- reqwest, or a scripted fake for offline tests

pub mod client;
pub mod config;
pub mod desk;
pub mod error;
pub mod transport;

pub use client::{Client, Poll, Run, RunArtifact};
pub use config::{ApiKey, Config};
pub use desk::{apply_quotes, run_desk, DesignPacket, DeskOutcome, DeskReply, Quote, Sourcing};
pub use error::CreaoError;
