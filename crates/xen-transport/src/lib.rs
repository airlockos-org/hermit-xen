#![no_std]
//! Bounded SPSC transport. No shared Rust references or wire-format Rust enums.
//! The peer is authorized by the platform grant/event policy, not by a port,
//! grant reference, session number, or checksum. All received bytes are untrusted.
use core::{
    marker::PhantomData,
    ptr::NonNull,
    sync::atomic::{AtomicU32, Ordering},
};

#[cfg(feature = "linux")]
extern crate std;
#[cfg(all(feature = "linux", target_os = "linux"))]
pub mod linux;

#[cfg(all(feature = "hermit", target_os = "hermit"))]
pub mod hermit;

pub const PAGE_SIZE: usize = 4096;
pub const CAPACITY: u32 = 8;
pub const MAX_PAYLOAD: usize = 224;
const FRAME_SIZE: usize = 256;
const DATA: usize = 256;
const PROD: usize = 64;
const CONS: usize = 128;
pub const ALIVE_OFFSET: usize = 12;
const MAGIC: u32 = 0x48585043;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Full,
    Empty,
    Closed,
    Timeout,
    Malformed,
    Corrupt,
    TooLarge,
    Platform(i32),
}

/// A private, validated message. Explicit little-endian framing on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub kind: Kind,
    pub id: u64,
    len: usize,
    bytes: [u8; MAX_PAYLOAD],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Kind {
    Request = 1,
    Response = 2,
    Cancel = 3,
    Close = 4,
    Error = 5,
}
impl Message {
    pub fn new(kind: Kind, id: u64, bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_PAYLOAD {
            return Err(Error::TooLarge);
        }
        let mut m = Self {
            kind,
            id,
            len: bytes.len(),
            bytes: [0; MAX_PAYLOAD],
        };
        m.bytes[..bytes.len()].copy_from_slice(bytes);
        Ok(m)
    }
    pub fn payload(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
    fn encode(&self) -> [u8; FRAME_SIZE] {
        let mut f = [0; FRAME_SIZE];
        f[..4].copy_from_slice(&MAGIC.to_le_bytes());
        f[4..6].copy_from_slice(&1u16.to_le_bytes());
        f[6..8].copy_from_slice(&(self.kind as u16).to_le_bytes());
        f[8..12].copy_from_slice(&(self.len as u32).to_le_bytes());
        f[16..24].copy_from_slice(&self.id.to_le_bytes());
        f[32..32 + self.len].copy_from_slice(self.payload());
        let checksum = checksum(&f);
        f[24..28].copy_from_slice(&checksum.to_le_bytes());
        f
    }
    fn decode(f: &[u8; FRAME_SIZE]) -> Result<Self, Error> {
        let u32_at = |n| u32::from_le_bytes(f[n..n + 4].try_into().unwrap());
        if u32_at(0) != MAGIC
            || f[4..6] != 1u16.to_le_bytes()
            || u32_at(12) != 0
            || u32_at(28) != 0
            || u32_at(24) != checksum(f)
        {
            return Err(Error::Malformed);
        }
        let kind = match u16::from_le_bytes(f[6..8].try_into().unwrap()) {
            1 => Kind::Request,
            2 => Kind::Response,
            3 => Kind::Cancel,
            4 => Kind::Close,
            5 => Kind::Error,
            _ => return Err(Error::Malformed),
        };
        let len = u32_at(8) as usize;
        if len > MAX_PAYLOAD || f[32 + len..].iter().any(|b| *b != 0) {
            return Err(Error::Malformed);
        }
        Self::new(
            kind,
            u64::from_le_bytes(f[16..24].try_into().unwrap()),
            &f[32..32 + len],
        )
    }
}
fn checksum(f: &[u8; FRAME_SIZE]) -> u32 {
    // Accidental corruption / inconsistent snapshots only; never authentication.
    f.iter()
        .enumerate()
        .filter(|(i, _)| !(24..28).contains(i))
        .fold(0x811c9dc5u32, |v, (_, b)| {
            (v ^ u32::from(*b)).wrapping_mul(0x01000193)
        })
}

/// Raw shared memory capability, tied to its owning backend by Channel.
/// Deliberately !Send/!Sync; one producer and one consumer per queue.
#[derive(Clone, Copy)]
pub struct Page {
    ptr: NonNull<u8>,
    _single: PhantomData<*mut ()>,
}
impl Page {
    /// # Safety
    /// A page-aligned, coherently mapped page must remain valid for every use.
    /// No ordinary Rust references may alias accesses through this capability.
    pub unsafe fn from_raw(ptr: *mut u8) -> Result<Self, Error> {
        let ptr = NonNull::new(ptr).ok_or(Error::Corrupt)?;
        if ptr.as_ptr().addr() % PAGE_SIZE != 0 {
            return Err(Error::Corrupt);
        }
        Ok(Self {
            ptr,
            _single: PhantomData,
        })
    }
    /// Raw address for platform/FFI integration. Dereferencing requires the
    /// mapping lifetime and synchronization contracts of `from_raw`.
    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }
    fn atomic(&self, offset: usize) -> &AtomicU32 {
        // All counter locations are aligned, initialized u32 shared words.
        unsafe { AtomicU32::from_ptr(self.ptr.as_ptr().add(offset).cast()) }
    }
    pub fn load(&self, offset: usize) -> u32 {
        assert!(matches!(
            offset,
            0 | 4 | 8 | 16 | 20 | 24 | 28 | PROD | CONS
        ));
        self.atomic(offset).load(Ordering::Acquire)
    }
    pub fn store(&self, offset: usize, value: u32) {
        assert!(matches!(offset, 16 | 20));
        self.atomic(offset).store(value, Ordering::Release);
    }
    pub fn alive(&self) -> bool {
        unsafe { self.ptr.as_ptr().add(ALIVE_OFFSET).read_volatile() != 0 }
    }
    pub fn close(&self) {
        unsafe {
            self.ptr.as_ptr().add(ALIVE_OFFSET).write_volatile(0);
        }
    }
    /// # Safety
    /// Exclusive page ownership before publishing any grant or queue endpoint.
    pub unsafe fn initialize(&self, session: u64) {
        unsafe {
            self.ptr.as_ptr().write_bytes(0, PAGE_SIZE);
        }
        self.atomic(0).store(MAGIC, Ordering::Relaxed);
        self.atomic(4).store(1, Ordering::Relaxed);
        self.atomic(8).store(CAPACITY, Ordering::Relaxed);
        self.atomic(24).store(session as u32, Ordering::Relaxed);
        self.atomic(28)
            .store((session >> 32) as u32, Ordering::Relaxed);
        unsafe {
            self.ptr.as_ptr().add(ALIVE_OFFSET).write_volatile(1);
        }
        core::sync::atomic::fence(Ordering::Release);
    }
    pub fn validate(&self, session: u64) -> Result<(), Error> {
        if self.load(0) != MAGIC
            || self.load(4) != 1
            || self.load(8) != CAPACITY
            || self.load(24) != session as u32
            || self.load(28) != (session >> 32) as u32
        {
            return Err(Error::Corrupt);
        }
        Ok(())
    }
}

pub struct Producer {
    page: Page,
    next: u32,
}
pub struct Consumer {
    page: Page,
    next: u32,
}
impl Producer {
    /// # Safety
    /// Exactly one producer, on a freshly initialized queue, for this lifetime.
    pub unsafe fn new(page: Page) -> Self {
        Self { page, next: 0 }
    }
    pub fn try_send(&mut self, message: &Message) -> Result<(), Error> {
        self.send_frame(&message.encode())
    }
    fn send_frame(&mut self, frame: &[u8; FRAME_SIZE]) -> Result<(), Error> {
        if self.page.load(PROD) != self.next {
            return Err(Error::Corrupt);
        }
        let used = self.next.wrapping_sub(self.page.load(CONS));
        if used > CAPACITY {
            return Err(Error::Corrupt);
        }
        if used == CAPACITY {
            return Err(Error::Full);
        }
        let offset = DATA + (self.next % CAPACITY) as usize * FRAME_SIZE;
        for (i, b) in frame.iter().enumerate() {
            unsafe {
                self.page.ptr.as_ptr().add(offset + i).write_volatile(*b);
            }
        }
        self.next = self.next.wrapping_add(1);
        self.page.atomic(PROD).store(self.next, Ordering::Release);
        Ok(())
    }
    #[cfg(test)]
    fn inject_malformed(&mut self) -> Result<(), Error> {
        self.send_frame(&[0xff; FRAME_SIZE])
    }
}
impl Consumer {
    /// # Safety
    /// Exactly one consumer, on a freshly initialized queue, for this lifetime.
    pub unsafe fn new(page: Page) -> Self {
        Self { page, next: 0 }
    }
    pub fn try_recv(&mut self) -> Result<Message, Error> {
        if self.page.load(CONS) != self.next {
            return Err(Error::Corrupt);
        }
        let used = self.page.load(PROD).wrapping_sub(self.next);
        if used > CAPACITY {
            return Err(Error::Corrupt);
        }
        if used == 0 {
            return Err(if self.page.alive() {
                Error::Empty
            } else {
                Error::Closed
            });
        }
        let offset = DATA + (self.next % CAPACITY) as usize * FRAME_SIZE;
        let mut private = [0u8; FRAME_SIZE];
        for (i, b) in private.iter_mut().enumerate() {
            *b = unsafe { self.page.ptr.as_ptr().add(offset + i).read_volatile() };
        }
        self.next = self.next.wrapping_add(1);
        self.page.atomic(CONS).store(self.next, Ordering::Release);
        // Decode only private storage. A malicious peer cannot cause a TOCTOU
        // length to become a Rust slice bound or an invalid enum/reference.
        Message::decode(&private)
    }
}

/// Platform capability, acquired through an explicit authorized-peer policy.
/// # Safety
/// Pages returned by pages() stay mapped/pinned until close/drop, including on
/// errors. Implementations must quarantine memory when revocation remains busy.
/// Both pages must be fresh queues with unique producer/consumer ownership;
/// pages() must not expose them to other queue endpoints during a Channel lifetime.
pub unsafe trait Backend {
    /// # Safety
    /// Caller must keep the backend open and preserve exclusive queue endpoints
    /// until all returned page capabilities are no longer used.
    unsafe fn pages(&self) -> (Page, Page);
    fn now_ns(&self) -> u64;
    fn generation(&self) -> u32;
    fn notify(&self) -> Result<(), Error>;
    fn wait(&mut self, observed: u32, deadline_ns: u64) -> Result<(), Error>;
    fn close(&mut self, deadline_ns: u64) -> Result<(), Error>;
}
pub struct Channel<B: Backend> {
    backend: B,
    tx: Producer,
    rx: Consumer,
    closed: bool,
    full_waits: u64,
}
impl<B: Backend> Channel<B> {
    pub fn new(backend: B) -> Self {
        let (tx, rx) = unsafe { backend.pages() };
        Self {
            backend,
            tx: unsafe { Producer::new(tx) },
            rx: unsafe { Consumer::new(rx) },
            closed: false,
            full_waits: 0,
        }
    }
    pub fn send_typed<T: Wire>(&mut self, id: u64, value: &T, deadline: u64) -> Result<(), Error> {
        let mut bytes = [0; MAX_PAYLOAD];
        let len = value.encode(&mut bytes)?;
        if len > MAX_PAYLOAD {
            return Err(Error::TooLarge);
        }
        self.send(&Message::new(T::KIND, id, &bytes[..len])?, deadline)
    }
    pub fn recv_typed<T: Wire>(&mut self, deadline: u64) -> Result<(u64, T), Error> {
        let m = self.recv(deadline)?;
        if m.kind != T::KIND {
            return Err(Error::Malformed);
        }
        Ok((m.id, T::decode(m.payload())?))
    }
    pub fn full_waits(&self) -> u64 {
        self.full_waits
    }
    pub fn now_ns(&self) -> u64 {
        self.backend.now_ns()
    }
    pub fn deadline_after(&self, duration_ns: u64) -> u64 {
        self.now_ns().saturating_add(duration_ns)
    }
    pub fn send(&mut self, message: &Message, deadline_ns: u64) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        loop {
            let observed = self.backend.generation();
            if !self.rx.page.alive() {
                return Err(Error::Closed);
            }
            match self.tx.try_send(message) {
                Ok(()) => return self.backend.notify(),
                Err(Error::Full) => {
                    self.full_waits += 1;
                    self.backend.notify()?;
                    self.backend.wait(observed, deadline_ns)?;
                }
                Err(e) => return Err(e),
            }
        }
    }
    /// Publish without waiting so a stalled peer cannot block unrelated endpoints.
    pub fn try_send(&mut self, message: &Message) -> Result<(), Error> {
        if self.closed || !self.rx.page.alive() {
            return Err(Error::Closed);
        }
        self.tx.try_send(message)?;
        self.backend.notify()
    }
    pub fn recv(&mut self, deadline_ns: u64) -> Result<Message, Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        loop {
            let observed = self.backend.generation();
            match self.rx.try_recv() {
                Err(Error::Empty) => self.backend.wait(observed, deadline_ns)?,
                result @ (Ok(_) | Err(Error::Malformed)) => {
                    self.backend.notify()?;
                    return result;
                }
                Err(e) => return Err(e),
            }
        }
    }
    /// Receive without waiting, allowing a single task to fairly service several peers.
    pub fn try_recv(&mut self) -> Result<Message, Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        match self.rx.try_recv() {
            result @ (Ok(_) | Err(Error::Malformed)) => {
                self.backend.notify()?;
                result
            }
            result => result,
        }
    }
    pub fn close(&mut self, deadline_ns: u64) -> Result<(), Error> {
        if !self.closed {
            self.closed = true;
            unsafe { self.backend.pages() }.0.close();
            let _ = self.backend.notify();
        }
        self.backend.close(deadline_ns)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    #[repr(C, align(4096))]
    struct Memory([u8; 4096]);
    #[test]
    fn wrap_full_and_malformed() {
        let mut memory = Memory([0; 4096]);
        let p = unsafe { Page::from_raw(memory.0.as_mut_ptr()).unwrap() };
        unsafe {
            p.initialize(7);
        }
        p.validate(7).unwrap();
        assert_eq!(p.validate(8), Err(Error::Corrupt));
        let mut tx = unsafe { Producer::new(p) };
        let mut rx = unsafe { Consumer::new(p) };
        tx.next = u32::MAX - 3;
        rx.next = tx.next;
        p.atomic(PROD).store(tx.next, Ordering::Relaxed);
        p.atomic(CONS).store(tx.next, Ordering::Relaxed);
        for round in 0..64 {
            for id in 0..CAPACITY {
                tx.try_send(
                    &Message::new(Kind::Request, u64::from(round * CAPACITY + id), b"hello")
                        .unwrap(),
                )
                .unwrap();
            }
            assert_eq!(
                tx.try_send(&Message::new(Kind::Close, 0, b"").unwrap()),
                Err(Error::Full)
            );
            for id in 0..CAPACITY {
                let m = rx.try_recv().unwrap();
                assert_eq!(m.id, u64::from(round * CAPACITY + id));
                assert_eq!(m.payload(), b"hello");
            }
        }
        tx.inject_malformed().unwrap();
        assert_eq!(rx.try_recv(), Err(Error::Malformed));
        assert_eq!(rx.try_recv(), Err(Error::Empty));
        p.atomic(PROD)
            .store(rx.next.wrapping_add(CAPACITY + 1), Ordering::Release);
        assert_eq!(rx.try_recv(), Err(Error::Corrupt));
    }
    #[test]
    fn every_wire_byte_is_validated_without_panicking() {
        let frame = Message::new(Kind::Request, 42, b"data").unwrap().encode();
        for i in 0..FRAME_SIZE {
            let mut bad = frame;
            bad[i] ^= 0xff;
            assert!(Message::decode(&bad).is_err());
        }
        assert_eq!(
            Message::new(Kind::Request, 0, &[0; MAX_PAYLOAD + 1]),
            Err(Error::TooLarge)
        );
    }
}

/// Implement for application-owned request/response schemas. Decoder inputs are
/// private, bounded bytes. This trait does not grant permission to a request.
pub trait Wire: Sized {
    const KIND: Kind;
    fn encode(&self, out: &mut [u8; MAX_PAYLOAD]) -> Result<usize, Error>;
    fn decode(bytes: &[u8]) -> Result<Self, Error>;
}

/// Peer identity selected by the caller's trusted provisioning mechanism.
/// This value is an address, not evidence of authorization. Never construct it
/// from an untrusted message without an independent identity/policy decision.
#[derive(Debug, Clone, Copy)]
pub struct Peer {
    pub domain: u16,
}
/// Connection offer exchanged through an authenticated provisioning channel.
/// The session is a freshness check, not a secret or credential.
#[derive(Debug, Clone, Copy)]
pub struct Offer {
    pub port: u32,
    pub grant: u32,
    pub session: u64,
}
