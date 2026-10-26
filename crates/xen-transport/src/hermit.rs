//! Hermit adaptation of the transport capability; Xen ABI stays inside the kernel.
use crate::{Backend, Channel, Error, Offer, Page, Peer};
unsafe extern "C" {
    fn sys_xen_open(peer: u16) -> i32;
    fn sys_xen_tx() -> usize;
    fn sys_xen_port() -> u32;
    fn sys_xen_publish() -> i32;
    fn sys_xen_map(gref: u32) -> usize;
    fn sys_xen_notify() -> i32;
    fn sys_xen_generation() -> u32;
    fn sys_xen_now_ns() -> u64;
    fn sys_xen_wait(observed: u32, deadline: u64) -> i32;
    fn sys_xen_close() -> i32;
}
fn checked(code: i32) -> Result<(), Error> {
    match code {
        0 => Ok(()),
        -11 => Ok(()),
        -110 => Err(Error::Timeout),
        e => Err(Error::Platform(e)),
    }
}
pub struct Hermit {
    tx: Page,
    rx: Page,
    closed: bool,
}
struct Opening;
impl Drop for Opening {
    fn drop(&mut self) {
        let _ = unsafe { sys_xen_close() };
    }
}

impl Hermit {
    pub fn connect(
        peer: Peer,
        timeout_ns: u64,
        offer: impl FnOnce(Offer),
    ) -> Result<Channel<Self>, Error> {
        unsafe {
            checked(sys_xen_open(peer.domain))?;
        }
        let opening = Opening;
        let tx = unsafe { Page::from_raw(sys_xen_tx() as *mut u8)? };
        let session = unsafe { sys_xen_now_ns() };
        unsafe {
            tx.initialize(session);
        }
        let grant = unsafe { sys_xen_publish() };
        if grant < 0 {
            return Err(Error::Platform(grant));
        }
        offer(Offer {
            port: unsafe { sys_xen_port() },
            grant: grant as u32,
            session,
        });
        let deadline = session.saturating_add(timeout_ns);
        loop {
            let generation = unsafe { sys_xen_generation() };
            if tx.load(20) == 1 {
                break;
            }
            if let Err(e) = checked(unsafe { sys_xen_wait(generation, deadline) }) {
                unsafe {
                    sys_xen_close();
                }
                return Err(e);
            }
        }
        let rx = unsafe { Page::from_raw(sys_xen_map(tx.load(16)) as *mut u8)? };
        rx.validate(session)?;
        core::mem::forget(opening);
        Ok(Channel::new(Self {
            tx,
            rx,
            closed: false,
        }))
    }
}
unsafe impl Backend for Hermit {
    unsafe fn pages(&self) -> (Page, Page) {
        (self.tx, self.rx)
    }
    fn now_ns(&self) -> u64 {
        unsafe { sys_xen_now_ns() }
    }
    fn generation(&self) -> u32 {
        unsafe { sys_xen_generation() }
    }
    fn notify(&self) -> Result<(), Error> {
        checked(unsafe { sys_xen_notify() })
    }
    fn wait(&mut self, observed: u32, deadline: u64) -> Result<(), Error> {
        checked(unsafe { sys_xen_wait(observed, deadline) })
    }
    fn close(&mut self, deadline: u64) -> Result<(), Error> {
        loop {
            let generation = self.generation();
            match unsafe { sys_xen_close() } {
                0 => {
                    self.closed = true;
                    return Ok(());
                }
                -16 => {
                    self.wait(
                        generation,
                        self.now_ns().saturating_add(10_000_000).min(deadline),
                    )
                    .or_else(|e| {
                        if e == Error::Timeout && self.now_ns() < deadline {
                            Ok(())
                        } else {
                            Err(e)
                        }
                    })?;
                }
                e => return Err(Error::Platform(e)),
            }
        }
    }
}
impl Drop for Hermit {
    fn drop(&mut self) {
        if !self.closed {
            let _ = unsafe { sys_xen_close() };
        }
    }
}
