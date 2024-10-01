use std::ptr;

use libc::{PROT_EXEC, PROT_READ, PROT_WRITE};

#[cfg(target_arch = "aarch64")]
pub unsafe fn patch(target: *mut libc::c_uchar, hook_fn: *mut libc::c_void) -> [u8; 16] {
    let code: [u8; 8] = [0x43, 0, 0, 0x58, 0x60, 0, 0x1f, 0xd6];
    // magic value = code size (8) + addr size (8)
    let backup = ptr::read_unaligned(target as *const [u8; 16]);
    ptr::write(target as *mut [u8; 8], code);
    let ptr = target.offset(8) as *mut fn();
    *ptr = core::mem::transmute(hook_fn);
    return backup;
}

pub unsafe fn setup_hook(
    orig_fn: *mut libc::c_void,
    hook_fn: *mut libc::c_void,
) -> Option<[u8; 16]> {
    let pa_addr = page_align_addr(orig_fn);
    libc::mprotect(
        pa_addr,
        page_size::get(),
        PROT_READ | PROT_WRITE | PROT_EXEC,
    );
    let result = patch(orig_fn as *mut libc::c_uchar, hook_fn);
    let origptr = orig_fn as *const libc::c_void;
    clear_cache::clear_cache(origptr, origptr.offset(16));
    libc::mprotect(pa_addr, page_size::get(), PROT_READ | PROT_EXEC);
    Some(result)
}

pub unsafe fn unsetup_hook(orig_fn: *mut libc::c_void, orig_code: [u8; 16]) {
    let pa_addr = page_align_addr(orig_fn);
    libc::mprotect(
        pa_addr,
        page_size::get(),
        PROT_READ | PROT_WRITE | PROT_EXEC,
    );
    ptr::write_unaligned(orig_fn as *mut [u8; 16], orig_code);

    let origptr = orig_fn as *const libc::c_void;
    clear_cache::clear_cache(origptr, origptr.offset(16));
    libc::mprotect(pa_addr, page_size::get(), PROT_READ | PROT_EXEC);
}
fn page_align_addr(addr: *mut libc::c_void) -> *mut libc::c_void {
    (addr as usize & !(page_size::get() - 1)) as *mut libc::c_void
}
