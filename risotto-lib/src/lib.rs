//! Decode BMP (RFC 7854) messages into collector events.
//!
//! The library is stateless: each BMP message becomes one or more [`event::Event`]s
//! mirroring the BMP message types. Reconstructing routing state (RIBs, peer sessions)
//! is left to downstream consumers, which receive Peer Up/Down, Initiation and
//! Termination events alongside Route Monitoring.

pub mod event;
pub mod route_monitoring;
pub mod session;

pub use event::{Event, EventBody, MessageType};
pub use session::{decode_bmp_message, BmpSession};
