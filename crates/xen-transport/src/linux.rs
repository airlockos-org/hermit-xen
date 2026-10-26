//! Linux userspace grant/event adapter. Requires access to /dev/xen devices.
//! Caller provisioning must authorize the peer before accepting an offer.
use crate::{ALIVE_OFFSET, Backend, Error, Offer, Page, Peer};
use std::{io, os::fd::AsRawFd, time::Instant};
mod event;
mod grant;
use event::Events as UnixEventChannelInterface;
use grant::GrantPage;
fn platform(e: io::Error) -> Error {
    Error::Platform(e.raw_os_error().unwrap_or(5))
}
pub struct Linux {
    tx: Option<GrantPage>,
    rx: Option<GrantPage>,
    events: UnixEventChannelInterface,
    port: u32,
    epoch: Instant,
    generation: u32,
    closed: bool,
}
impl Linux {
    /// Map an offer obtained from the caller's trusted provisioning authority.
    pub fn open(peer: Peer, offer: Offer) -> Result<Self, Error> {
        let domid = peer.domain;
        let Offer {
            port,
            grant,
            session,
        } = offer;
        let mut events = UnixEventChannelInterface::new().map_err(platform)?;
        let port = events.bind_interdomain(domid, port).map_err(platform)?;
        let rx = GrantPage::map(domid, grant).map_err(platform)?;
        let tx = GrantPage::allocate(domid).map_err(platform)?;
        let outgoing = unsafe { Page::from_raw(tx.address())? };
        // Linux owns the new allocation; grant ID is not published to the peer yet.
        unsafe {
            outgoing.initialize(session);
        }
        tx.notify_on_unmap(ALIVE_OFFSET as u64, port)
            .map_err(platform)?;
        let incoming = unsafe { Page::from_raw(rx.address())? };
        incoming.validate(session)?;
        incoming.store(16, tx.reference());
        incoming.store(20, 1);
        events.send(port).map_err(platform)?;
        Ok(Self {
            tx: Some(tx),
            rx: Some(rx),
            events,
            port,
            epoch: Instant::now(),
            generation: 0,
            closed: false,
        })
    }
}
unsafe impl Backend for Linux {
    unsafe fn pages(&self) -> (Page, Page) {
        unsafe {
            (
                Page::from_raw(self.tx.as_ref().unwrap().address()).unwrap(),
                Page::from_raw(self.rx.as_ref().unwrap().address()).unwrap(),
            )
        }
    }
    fn now_ns(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }
    fn generation(&self) -> u32 {
        self.generation
    }
    fn notify(&self) -> Result<(), Error> {
        self.events.send(self.port).map_err(platform)
    }
    fn wait(&mut self, observed: u32, deadline: u64) -> Result<(), Error> {
        if self.generation != observed {
            return Ok(());
        }
        loop {
            let now = self.now_ns();
            if now >= deadline {
                return Err(Error::Timeout);
            }
            let millis = (deadline - now).div_ceil(1_000_000).min(i32::MAX as u64) as i32;
            let mut poll = libc::pollfd {
                fd: self.events.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut poll, 1, millis) };
            if ready < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(platform(e));
            }
            if ready == 0 {
                return Err(Error::Timeout);
            }
            if poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return Err(Error::Closed);
            }
            let port = self.events.pending().map_err(platform)?;
            if port != self.port {
                return Err(Error::Corrupt);
            }
            self.events.unmask(port).map_err(platform)?;
            self.generation = self.generation.wrapping_add(1);
            return Ok(());
        }
    }
    fn close(&mut self, _deadline: u64) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        if let Some(page) = self.rx.as_mut() {
            page.close().map_err(platform)?;
        }
        self.rx = None;
        if let Some(page) = self.tx.as_mut() {
            page.close().map_err(platform)?;
        }
        self.tx = None;
        let _ = self.events.send(self.port);
        self.events.unbind(self.port).map_err(platform)?;
        self.closed = true;
        Ok(())
    }
}
impl Drop for Linux {
    fn drop(&mut self) {
        let _ = self.close(0);
    }
}
