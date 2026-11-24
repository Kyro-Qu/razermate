//! Basilisk V3 control protocol.
//!
//! Pure logic: report layout, CRC, command builders, macro codec. No OS calls,
//! so everything here is unit-testable without a mouse plugged in.
//!
//! See `docs/PROTOCOL.md` for the wire specification and the evidence behind
//! each constant.

pub mod commands;
pub mod keys;
pub mod macro_codec;
pub mod report;

pub use report::{Report, Status, ARG_LEN, REPORT_LEN, TRANSACTION_ID};

/// USB identifiers of the supported device.
pub const VENDOR_ID: u16 = 0x1532;
pub const PRODUCT_ID: u16 = 0x0099;
/// The vendor control interface.
pub const CONTROL_INTERFACE: u8 = 3;
pub const PRODUCT_NAME: &str = "Razer Basilisk V3";
