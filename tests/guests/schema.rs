//! Test-only request/response schema.
use xen_transport::{Error, Kind, MAX_PAYLOAD, Wire};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Echo = 0,
    Mask = 1,
    Close = 2,
    Cancel = 3,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub operation: Operation,
    pub value: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Response {
    pub value: u64,
    pub timestamp_ns: u64,
}
impl Wire for Request {
    const KIND: Kind = Kind::Request;
    fn encode(&self, out: &mut [u8; MAX_PAYLOAD]) -> Result<usize, Error> {
        out[0] = self.operation as u8;
        out[8..16].copy_from_slice(&self.value.to_le_bytes());
        Ok(16)
    }
    fn decode(b: &[u8]) -> Result<Self, Error> {
        if b.len() != 16 || b[1..8].iter().any(|v| *v != 0) {
            return Err(Error::Malformed);
        }
        let operation = match b[0] {
            0 => Operation::Echo,
            1 => Operation::Mask,
            2 => Operation::Close,
            3 => Operation::Cancel,
            _ => return Err(Error::Malformed),
        };
        Ok(Self {
            operation,
            value: u64::from_le_bytes(b[8..16].try_into().unwrap()),
        })
    }
}
impl Wire for Response {
    const KIND: Kind = Kind::Response;
    fn encode(&self, out: &mut [u8; MAX_PAYLOAD]) -> Result<usize, Error> {
        out[..8].copy_from_slice(&self.value.to_le_bytes());
        out[8..16].copy_from_slice(&self.timestamp_ns.to_le_bytes());
        Ok(16)
    }
    fn decode(b: &[u8]) -> Result<Self, Error> {
        if b.len() != 16 {
            return Err(Error::Malformed);
        }
        Ok(Self {
            value: u64::from_le_bytes(b[..8].try_into().unwrap()),
            timestamp_ns: u64::from_le_bytes(b[8..16].try_into().unwrap()),
        })
    }
}
