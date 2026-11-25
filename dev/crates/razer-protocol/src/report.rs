//! Wire format for the Basilisk V3 vendor control interface (MI_03).
//!
//! 90-byte feature report, plus a leading report id `0x00` on the wire.

/// Report id used by every feature report on this device.
pub const REPORT_ID: u8 = 0x00;
/// Length of the structure itself (excluding the report id).
pub const REPORT_LEN: usize = 90;
/// Length of the argument block inside the structure.
pub const ARG_LEN: usize = 80;
/// Fixed transaction id for the Basilisk V3.
pub const TRANSACTION_ID: u8 = 0x1F;
/// Byte offset of the CRC inside the structure.
pub const CRC_OFFSET: usize = 88;

/// Offset of `command_class` inside the structure.
pub const OFF_CLASS: usize = 6;
/// Offset of `command_id` inside the structure.
pub const OFF_COMMAND: usize = 7;
/// Offset of the argument block inside the structure.
pub const OFF_ARGS: usize = 8;

/// Reply status. Anything other than `Ok` and `Busy` is a hard failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    New,
    /// The device is still chewing on the previous command; poll again.
    Busy,
    Ok,
    Fail,
    Timeout,
    NotSupported,
    Unknown(u8),
}

impl Status {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Status::New,
            1 => Status::Busy,
            2 => Status::Ok,
            3 => Status::Fail,
            4 => Status::Timeout,
            5 => Status::NotSupported,
            other => Status::Unknown(other),
        }
    }

    pub fn is_ok(self) -> bool {
        matches!(self, Status::Ok)
    }

    pub fn is_busy(self) -> bool {
        matches!(self, Status::Busy)
    }
}

/// A single request/reply frame.
#[derive(Clone, PartialEq, Eq)]
pub struct Report {
    pub status: Status,
    pub transaction_id: u8,
    pub remaining_packets: u16,
    pub protocol_type: u8,
    /// Length of the argument block that matters: on the way out, how many
    /// bytes are meaningful; on the way back, how many bytes are the reply.
    pub data_size: u8,
    pub command_class: u8,
    pub command_id: u8,
    pub arguments: [u8; ARG_LEN],
    pub crc: u8,
}

impl Report {
    /// Build a request. `data_size` is also the number of meaningful bytes in
    /// `arguments`, so the two can never drift apart.
    pub fn request(command_class: u8, command_id: u8, data_size: u8, args: &[u8]) -> Self {
        let mut arguments = [0u8; ARG_LEN];
        let n = args.len().min(ARG_LEN);
        arguments[..n].copy_from_slice(&args[..n]);
        let data_size = data_size.min(ARG_LEN as u8);
        // Pad up to data_size: the device expects the whole block, not just the
        // bytes we happened to pass.
        for slot in arguments.iter_mut().take(data_size as usize).skip(n) {
            *slot = 0;
        }
        Report {
            status: Status::New,
            transaction_id: TRANSACTION_ID,
            remaining_packets: 0,
            protocol_type: 0,
            data_size,
            command_class,
            command_id,
            arguments,
            crc: 0,
        }
    }

    /// XOR of bytes 2..=87.
    pub fn compute_crc(&self) -> u8 {
        let bytes = self.to_bytes_unsealed();
        bytes[2..CRC_OFFSET].iter().fold(0u8, |acc, b| acc ^ b)
    }

    fn to_bytes_unsealed(&self) -> [u8; REPORT_LEN] {
        let mut out = [0u8; REPORT_LEN];
        out[0] = match self.status {
            Status::New => 0,
            Status::Busy => 1,
            Status::Ok => 2,
            Status::Fail => 3,
            Status::Timeout => 4,
            Status::NotSupported => 5,
            Status::Unknown(v) => v,
        };
        out[1] = self.transaction_id;
        out[2..4].copy_from_slice(&self.remaining_packets.to_be_bytes());
        out[4] = self.protocol_type;
        out[5] = self.data_size;
        out[6] = self.command_class;
        out[7] = self.command_id;
        out[OFF_ARGS..OFF_ARGS + ARG_LEN].copy_from_slice(&self.arguments);
        out[CRC_OFFSET] = self.crc;
        out
    }

    /// Serialise with a freshly computed CRC.
    pub fn to_bytes(&self) -> [u8; REPORT_LEN] {
        let mut out = self.to_bytes_unsealed();
        let crc = out[2..CRC_OFFSET].iter().fold(0u8, |acc, b| acc ^ b);
        out[CRC_OFFSET] = crc;
        out
    }

    /// Serialise with the leading report id, ready for `HidD_SetFeature`.
    pub fn to_feature_buffer(&self) -> [u8; REPORT_LEN + 1] {
        let mut out = [0u8; REPORT_LEN + 1];
        out[1..].copy_from_slice(&self.to_bytes());
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < REPORT_LEN {
            return None;
        }
        let mut arguments = [0u8; ARG_LEN];
        arguments.copy_from_slice(&bytes[OFF_ARGS..OFF_ARGS + ARG_LEN]);
        Some(Report {
            status: Status::from_u8(bytes[0]),
            transaction_id: bytes[1],
            remaining_packets: u16::from_be_bytes([bytes[2], bytes[3]]),
            protocol_type: bytes[4],
            data_size: bytes[5],
            command_class: bytes[6],
            command_id: bytes[7],
            arguments,
            crc: bytes[CRC_OFFSET],
        })
    }

    /// The reply bytes for this command (the leading `data_size` bytes).
    pub fn reply(&self) -> &[u8] {
        let n = (self.data_size as usize).min(ARG_LEN);
        &self.arguments[..n]
    }

    /// CRC check on a received frame.
    pub fn crc_ok(&self) -> bool {
        let bytes = self.to_bytes_unsealed();
        let want = bytes[2..CRC_OFFSET].iter().fold(0u8, |acc, b| acc ^ b);
        want == self.crc
    }

    /// True when this frame answers the given request.
    pub fn answers(&self, request: &Report) -> bool {
        self.command_class == request.command_class && self.command_id == request.command_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_covers_bytes_two_to_eighty_seven() {
        let mut r = Report::request(0x00, 0x81, 4, &[]);
        r.arguments[0] = 0xAA;
        let b = r.to_bytes();
        let expect = b[2..88].iter().fold(0u8, |a, x| a ^ x);
        assert_eq!(b[88], expect);
    }

    #[test]
    fn roundtrip_preserves_everything() {
        let r = Report::request(0x0F, 0x82, 2, &[4, 0x0A]);
        let bytes = r.to_bytes();
        let back = Report::from_bytes(&bytes).unwrap();
        assert_eq!(back.command_class, 0x0F);
        assert_eq!(back.command_id, 0x82);
        assert_eq!(back.data_size, 2);
        assert_eq!(&back.arguments[..2], &[4, 0x0A]);
        assert!(back.crc_ok());
    }

    #[test]
    fn request_pads_arguments_to_data_size() {
        let r = Report::request(0x02, 0x0C, 10, &[1, 5, 0]);
        assert_eq!(r.data_size, 10);
        assert_eq!(&r.arguments[..10], &[1, 5, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn feature_buffer_has_leading_report_id() {
        let r = Report::request(0x05, 0x8A, 1, &[]);
        let buf = r.to_feature_buffer();
        assert_eq!(buf.len(), 91);
        assert_eq!(buf[0], REPORT_ID);
        assert_eq!(buf[7], 0x05);
        assert_eq!(buf[8], 0x8A);
    }

    #[test]
    fn transaction_id_is_fixed() {
        let r = Report::request(0x06, 0x8E, 14, &[]);
        assert_eq!(r.transaction_id, 0x1F);
    }

    #[test]
    fn answers_matches_class_and_id() {
        let req = Report::request(0x06, 0x8E, 14, &[]);
        let mut reply = req.clone();
        reply.status = Status::Ok;
        assert!(reply.answers(&req));
        let mut other = req.clone();
        other.command_id = 0x80;
        assert!(!other.answers(&req));
    }
}
