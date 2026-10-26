//! Linux evtchn UAPI adapter, following rust-vmm/xen's UnixEventChannelInterface.
//! See THIRD_PARTY.md for upstream attribution. No port is an authorization token.
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    os::fd::AsRawFd,
};

pub(super) struct Events(File);
impl Events {
    pub fn new() -> io::Result<Self> {
        Ok(Self(
            OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/xen/evtchn")?,
        ))
    }
    fn operation<T>(&self, number: u8, arg: &T) -> io::Result<u32> {
        let cmd = (size_of::<T>() << 16) | ((b'E' as usize) << 8) | usize::from(number);
        // Linux UAPI _IOC_NONE layouts: bind = two u32, notify/unbind = one u32.
        let result = unsafe { libc::ioctl(self.0.as_raw_fd(), cmd as _, arg as *const T) };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(result as u32)
    }
    pub fn bind_interdomain(&mut self, domain: u16, port: u32) -> io::Result<u32> {
        self.operation(1, &[u32::from(domain), port])
    }
    pub fn send(&self, port: u32) -> io::Result<()> {
        self.operation(4, &port).map(|_| ())
    }
    pub fn unbind(&mut self, port: u32) -> io::Result<()> {
        self.operation(3, &port).map(|_| ())
    }
    pub fn pending(&mut self) -> io::Result<u32> {
        let mut b = [0; 4];
        self.0.read_exact(&mut b)?;
        Ok(u32::from_ne_bytes(b))
    }
    pub fn unmask(&mut self, port: u32) -> io::Result<()> {
        self.0.write_all(&port.to_ne_bytes())
    }
}
impl AsRawFd for Events {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        self.0.as_raw_fd()
    }
}
