//! Deterministic kernel controls used only by the regression fixture.
use xen_transport::Error;
unsafe extern "C" {
    fn sys_xen_mask(mask: u32) -> i32;
    fn sys_xen_revoke_probe() -> i32;
}
pub fn mask_notifications(mask: bool) -> Result<(), Error> {
    match unsafe { sys_xen_mask(u32::from(mask)) } {
        0 => Ok(()),
        e => Err(Error::Platform(e)),
    }
}
pub fn verify_busy_revocation() -> Result<(), Error> {
    if unsafe { sys_xen_revoke_probe() } == -16 {
        Ok(())
    } else {
        Err(Error::Corrupt)
    }
}
