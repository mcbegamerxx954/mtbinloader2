use cxx::CxxString;
use std::{
    ffi::c_void,
    mem::{transmute, MaybeUninit},
    pin::Pin,
};
// Smart pointer for ResourceLocation
#[repr(transparent)]
pub struct ResourceLocation(*mut c_void);
impl Default for ResourceLocation {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceLocation {
    pub fn new() -> Self {
        unsafe { resource_location_init() }
    }
    pub fn get_path<'a>(&mut self) -> Pin<&'a mut CxxString> {
        // SAFETY: We just did not force it to be pin since then borrow checker gets angry
        unsafe {
            let ptr = resource_location_path(self.0);
            transmute(ptr)
        }
    }
}
impl Drop for ResourceLocation {
    fn drop(&mut self) {
        // SAFETY: We handle the scope so its good
        unsafe { resource_location_free(self.0) }
    }
}
// Linking against string.cpp
extern "C" {
    fn resource_location_init() -> ResourceLocation;
    fn resource_location_path(loc: *mut libc::c_void) -> *mut CxxString;
    fn resource_location_free(loc: *mut libc::c_void);
}
extern "C" {
    #[link_name = "cxxbridge1$cxx_string$init"]
    fn string_init(this: &mut MaybeUninit<CxxString>, ptr: *const u8, len: usize);
    #[link_name = "cxxbridge1$cxx_string$destroy"]
    fn string_destroy(this: &mut MaybeUninit<CxxString>);
}

#[repr(C)]
pub struct StackString {
    // Static assertions in cxx.cc validate that this is large enough and
    // aligned enough.
    space: MaybeUninit<[usize; 8]>,
}
impl AsRef<[u8]> for StackString {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            let this = &*self.space.as_ptr().cast::<MaybeUninit<CxxString>>();
            let cxxptr = &*this.as_ptr();
            cxxptr.as_bytes()
        }
    }
}
#[allow(missing_docs)]
impl StackString {
    pub const fn new() -> Self {
        Self {
            space: MaybeUninit::uninit(),
        }
    }

    pub unsafe fn init(&mut self, value: impl AsRef<[u8]>) -> Pin<&mut CxxString> {
        let value = value.as_ref();
        unsafe {
            let this = &mut *self.space.as_mut_ptr().cast::<MaybeUninit<CxxString>>();
            string_init(this, value.as_ptr(), value.len());
            Pin::new_unchecked(&mut *this.as_mut_ptr())
        }
    }
}

impl Drop for StackString {
    fn drop(&mut self) {
        unsafe {
            let this = &mut *self.space.as_mut_ptr().cast::<MaybeUninit<CxxString>>();
            string_destroy(this);
        }
    }
}
