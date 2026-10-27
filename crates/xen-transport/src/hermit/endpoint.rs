//! Independently owned endpoints for services with several authorized peers.
//! A trusted provisioner supplies peer identities; offers never authenticate callers.

use crate::{Backend, Channel, Error, Offer, Page, Peer};

unsafe extern "C" {
    fn sys_xen_connection_open(peer: u16) -> i32;
    fn sys_xen_connection_tx(handle: u32) -> usize;
    fn sys_xen_connection_port(handle: u32) -> u32;
    fn sys_xen_connection_publish(handle: u32) -> i32;
    fn sys_xen_connection_map(handle: u32, grant: u32) -> usize;
    fn sys_xen_connection_notify(handle: u32) -> i32;
    fn sys_xen_connection_close(handle: u32) -> i32;
}

/// Event generation shared by the single-vCPU service loop.
pub fn generation() -> u32 {
    unsafe { super::sys_xen_generation() }
}

/// Monotonic Xen nanoseconds used by every endpoint in this guest.
pub fn now_ns() -> u64 {
    unsafe { super::sys_xen_now_ns() }
}

/// Block until an event or deadline, preserving the check-before-wait race guard.
pub fn wait(observed: u32, deadline: u64) -> Result<(), Error> {
    super::checked(unsafe { super::sys_xen_wait(observed, deadline) })
}

/// An offered connection whose peer has not yet completed its grant handshake.
pub struct Pending {
    /// Kernel capability, removed exactly once when ownership enters a channel.
    handle: Option<u32>,
    /// Locally owned transmit page, initialized before its grant is published.
    tx: Page,
    /// Offer delivered only through the trusted provisioning interface.
    offer: Offer,
}

impl Pending {
    /// Grant an endpoint to one authenticated Xen peer without blocking on it.
    pub fn new(peer: Peer, session: u64) -> Result<Self, Error> {
        let handle = unsafe { sys_xen_connection_open(peer.domain) };
        if handle < 0 {
            return Err(Error::Platform(handle));
        }
        let handle = handle as u32;
        let result = (|| {
            let tx = unsafe { Page::from_raw(sys_xen_connection_tx(handle) as *mut u8)? };
            unsafe {
                tx.initialize(session);
            }
            let grant = unsafe { sys_xen_connection_publish(handle) };
            if grant < 0 {
                return Err(Error::Platform(grant));
            }
            Ok(Self {
                handle: Some(handle),
                tx,
                offer: Offer {
                    port: unsafe { sys_xen_connection_port(handle) },
                    grant: grant as u32,
                    session,
                },
            })
        })();
        if result.is_err() {
            let _ = unsafe { sys_xen_connection_close(handle) };
        }
        result
    }

    /// Return transport coordinates; the delivery path must authenticate the recipient.
    pub fn offer(&self) -> Offer {
        Offer {
            port: self.offer.port,
            grant: self.offer.grant,
            session: self.offer.session,
        }
    }

    /// Finish a peer's handshake once, returning a separately owned channel.
    pub fn accept(&mut self) -> Result<Option<Channel<Endpoint>>, Error> {
        let handle = self.handle.ok_or(Error::Closed)?;
        if self.tx.load(20) != 1 {
            return Ok(None);
        }
        let rx =
            unsafe { Page::from_raw(sys_xen_connection_map(handle, self.tx.load(16)) as *mut u8)? };
        rx.validate(self.offer.session)?;
        self.handle = None;
        Ok(Some(Channel::new(Endpoint {
            handle,
            tx: self.tx,
            rx,
            closed: false,
        })))
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(handle) = self.handle {
            self.tx.close();
            let _ = unsafe { sys_xen_connection_close(handle) };
        }
    }
}

/// One kernel endpoint; failed revocation quarantines its pages until guest teardown.
pub struct Endpoint {
    /// Generation-bearing kernel handle rejecting stale owners after slot reuse.
    handle: u32,
    /// Pinned transmit mapping owned by this connection.
    tx: Page,
    /// Foreign receive mapping valid only until successful closure.
    rx: Page,
    /// Whether closure has already revoked this capability.
    closed: bool,
}

unsafe impl Backend for Endpoint {
    unsafe fn pages(&self) -> (Page, Page) {
        (self.tx, self.rx)
    }
    fn now_ns(&self) -> u64 {
        now_ns()
    }
    fn generation(&self) -> u32 {
        generation()
    }
    fn notify(&self) -> Result<(), Error> {
        super::checked(unsafe { sys_xen_connection_notify(self.handle) })
    }
    fn wait(&mut self, observed: u32, deadline: u64) -> Result<(), Error> {
        wait(observed, deadline)
    }
    fn close(&mut self, _deadline: u64) -> Result<(), Error> {
        if !self.closed {
            let result = unsafe { sys_xen_connection_close(self.handle) };
            if result < 0 {
                return Err(Error::Platform(result));
            }
            self.closed = true;
        }
        Ok(())
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        if !self.closed {
            self.tx.close();
            let _ = unsafe { sys_xen_connection_close(self.handle) };
        }
    }
}
