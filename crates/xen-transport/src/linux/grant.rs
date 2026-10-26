//! RAII wrappers for Linux gntalloc/gntdev UAPIs. No foreign-memory references.

use std::{
    fs::{File, OpenOptions},
    io,
    os::fd::AsRawFd,
    ptr::NonNull,
};
const PAGE: usize = 4096;
#[repr(C)]
struct Allocate {
    domid: u16,
    flags: u16,
    count: u32,
    index: u64,
    gref: u32,
}
#[repr(C)]
struct Map {
    count: u32,
    pad: u32,
    index: u64,
    domid: u32,
    gref: u32,
}
#[repr(C)]
struct Release {
    index: u64,
    count: u32,
    pad: u32,
}
#[repr(C)]
struct Notify {
    index: u64,
    action: u32,
    port: u32,
}
fn ioctl<T>(fd: &File, n: u8, arg: &mut T) -> io::Result<()> {
    let cmd = ((size_of::<T>() as libc::c_ulong) << 16)
        | ((b'G' as libc::c_ulong) << 8)
        | libc::c_ulong::from(n);
    // SAFETY: C layout and sizes follow Linux include/uapi/xen/{gntalloc,gntdev}.h.
    if unsafe { libc::ioctl(fd.as_raw_fd(), cmd as _, arg as *mut T) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub struct GrantPage {
    file: File,
    ptr: NonNull<u8>,
    index: u64,
    reference: u32,
    owned: bool,
    mapped: bool,
    released: bool,
}
impl GrantPage {
    pub fn allocate(peer: u16) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/xen/gntalloc")?;
        let mut arg = Allocate {
            domid: peer,
            flags: 1,
            count: 1,
            index: 0,
            gref: 0,
        };
        ioctl(&file, 5, &mut arg)?;
        Self::map_file(file, arg.index, arg.gref, true)
    }
    pub fn map(peer: u16, reference: u32) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/xen/gntdev")?;
        let mut arg = Map {
            count: 1,
            pad: 0,
            index: 0,
            domid: u32::from(peer),
            gref: reference,
        };
        ioctl(&file, 0, &mut arg)?;
        Self::map_file(file, arg.index, reference, false)
    }
    fn map_file(file: File, index: u64, reference: u32, owned: bool) -> io::Result<Self> {
        let ptr = unsafe {
            libc::mmap(
                core::ptr::null_mut(),
                PAGE,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                index as libc::off_t,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            file,
            ptr: NonNull::new(ptr.cast()).unwrap(),
            index,
            reference,
            owned,
            mapped: true,
            released: false,
        })
    }
    pub fn address(&self) -> *mut u8 {
        assert!(self.mapped);
        self.ptr.as_ptr()
    }
    pub fn reference(&self) -> u32 {
        self.reference
    }
    /// Arrange kernel-driven peer-death notification, including process::exit
    /// and SIGKILL where Rust destructors do not run.
    pub fn notify_on_unmap(&self, byte_offset: u64, port: u32) -> io::Result<()> {
        if byte_offset >= PAGE as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unmap byte outside page",
            ));
        }
        let mut arg = Notify {
            index: self.index + byte_offset,
            action: 3,
            port,
        };
        ioctl(&self.file, 7, &mut arg)
    }
    pub fn close(&mut self) -> io::Result<()> {
        if self.mapped {
            if unsafe { libc::munmap(self.ptr.as_ptr().cast(), PAGE) } != 0 {
                return Err(io::Error::last_os_error());
            }
            self.mapped = false;
        }
        if !self.released {
            let mut arg = Release {
                index: self.index,
                count: 1,
                pad: 0,
            };
            ioctl(&self.file, if self.owned { 6 } else { 1 }, &mut arg)?;
            self.released = true;
        }
        Ok(())
    }
}
impl Drop for GrantPage {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
