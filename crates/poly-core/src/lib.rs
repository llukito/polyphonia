//! Shared types, traits, errors and binary formats for Polyphonia.
//!
//! Everything here is the contract between the crates. The formats are
//! specified in `docs/SPEC.md`; this crate is their reference implementation.
//! Owned by role 1 (lead): change only through a PR the lead approves.

pub mod envelope;
pub mod error;
pub mod payload;
pub mod traits;
pub mod types;

#[cfg(feature = "test-utils")]
pub mod test_utils;

pub use envelope::{Envelope, contact_id};
pub use error::PolyError;
pub use payload::{Payload, slot_pad, slot_unpad};
pub use traits::{Carrier, Cipher};
pub use types::{
    Algorithm, CarrierKind, CarrierOptions, ContactId, Identity, OpenRequest, Opened, SealRequest,
    Slot,
};
