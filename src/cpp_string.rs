use cxx::CxxString;
use std::{mem::MaybeUninit, pin::Pin};

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
    fn as_ref<'a>(&'a self) -> &'a [u8] {
        unsafe {
            let this = &*self.space.as_ptr().cast::<MaybeUninit<CxxString>>();
            let cxxptr = &*this.as_ptr();
            return cxxptr.as_bytes();
        }
    }
}
#[allow(missing_docs)]
impl StackString {
    pub fn new() -> Self {
        StackString {
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
