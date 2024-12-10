use std::{ffi::CStr, sync::OnceLock};
mod aasset;
mod hooking;
mod plthook;
use crate::plthook::replace_plt_functions;
use core::{mem::transmute, slice};
use cxx::CxxString;
use hooking::{setup_hook, unsetup_hook, BACKUP_LEN};
use libc::{c_void, PT_LOAD};
use lightningscanner::Scanner;
use plt_rs::DynamicLibrary;

// Byte pattern of ResourcePackManager constructor
#[cfg(target_arch = "aarch64")]
const RPMC_PATTERNS: [&str; 2] = [
    //1.19.50-1.21.44
    "FF 03 03 D1 FD 7B 07 A9 FD C3 01 91 F9 43 00 F9 F8 5F 09 A9 F6 57 0A A9 F4 4F 0B A9 59 D0 3B D5 F6 03 03 2A 28 17 40 F9 F5 03 02 AA F3 03 00 AA A8 83 1F F8 28 10 40 F9",
    //1.21.60.21preview
    "FF 83 02 D1 FD 7B 06 A9 FD 83 01 91 F8 5F 07 A9 F6 57 08 A9 F4 4F 09 A9 58 D0 3B D5 F6 03 03 2A 08 17 40 F9 F5 03 02 AA F3 03 00 AA A8 83 1F F8 28 10 40 F9 28 01 00 B4",
];
#[cfg(target_arch = "arm")]
const RPMC_PATTERNS: [&str; 1] = [
    //1.19.50-1.21.44
    "F0 B5 03 AF 2D E9 00 ?? ?? B0 05 46 ?? 48 98 46 92 46 78 44 00 68 00 68 ?? 90 08 69",
];
// A opaque object to ResourceLocation
#[repr(C)]
pub struct ResourceLocation {
    _data: [u8; 0],
    _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
}
impl ResourceLocation {
    // Create one from a string, copying it
    pub fn from_str(str: &str) -> *mut ResourceLocation {
        unsafe { resource_location_init(str.as_ptr(), str.len()) }
    }
    // You must never use this struct again once this is called
    pub unsafe fn free(loc: *mut ResourceLocation) {
        unsafe { resource_location_free(loc) }
    }
}
extern "C" {
    fn resource_location_init(
        strptr: *const libc::c_char,
        size: libc::size_t,
    ) -> *mut ResourceLocation;
    fn resource_location_free(loc: *mut ResourceLocation);
}
// Setup for the log crate
pub fn setup_logging() {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Trace),
    );
}
#[ctor::ctor]
fn main() {
    setup_logging();
    log::info!("Starting");
    let range = dumb_callback().unwrap();
    // Pattern from mrwang
    let addr = find_signatures(&RPMC_PATTERNS, range).expect("No signsture was found");
    log::info!("hooking rpm");
    let result = unsafe { setup_hook(addr as *mut _, hook_rpm_ctor as *const _) };
    // Unwrapping is safe because this only happens once
    BACKUP
        .set(MemBackup {
            backup_bytes: result,
            original_func_ptr: addr as *mut _,
        })
        .unwrap();
    log::info!("hooking aasset");
    // Likely mc will also not start up the resource pack system before we get to the main screen
    // so we make use of that to reduce startup time by making the less important hooks
    // be setup in the background
    std::thread::spawn(hook_aaset);
}
fn find_signatures(signatures: &[&str], range: (usize, usize)) -> Option<*const u8> {
    for sig in signatures {
        let scanner = Scanner::new(sig);
        let addr = unsafe { scanner.find(None, range.0 as *const u8, range.1) };
        let addr = addr.get_addr();
        if addr.is_null() {
            log::error!("cannot find signature");
            continue;
        }
        #[cfg(target_arch = "arm")]
        // Needed for reasons
        let addr = unsafe { addr.offset(1) };
        return Some(addr);
    }
    None
}
// Setup asset hooks
pub fn hook_aaset() {
    const LIBNAME: &str = "libminecraftpe.so";
    let lib_entry = match find_lib(LIBNAME) {
        Some(lib) => lib,
        None => {
            log::info!("Cannot find minecraftpe?");
            panic!();
        }
    };
    let dyn_lib = match DynamicLibrary::initialize(lib_entry) {
        Ok(lib) => lib,
        Err(e) => {
            log::error!("failed to initilize dyn_lib: {e}");
            panic!();
        }
    };
    // Hook all aassetmanager functions
    replace_plt_functions(
        &dyn_lib,
        [
            ("AAssetManager_open", aasset::asset_open as *const _),
            ("AAsset_read", aasset::asset_read as *const _),
            ("AAsset_close", aasset::asset_close as *const _),
            ("AAsset_seek", aasset::asset_seek as *const _),
            ("AAsset_seek64", aasset::asset_seek64 as *const _),
            ("AAsset_getLength", aasset::asset_length as *const _),
            ("AAsset_getLength64", aasset::asset_length64 as *const _),
            (
                "AAsset_getRemainingLength",
                aasset::asset_remaining as *const _,
            ),
            (
                "AAsset_getRemainingLength64",
                aasset::asset_remaining64 as *const _,
            ),
            (
                "AAsset_openFileDescriptor",
                aasset::asset_fd_dummy as *const _,
            ),
            (
                "AAsset_openFileDescriptor64",
                aasset::asset_fd_dummy64 as *const _,
            ),
            ("AAsset_getBuffer", aasset::asset_get_buffer as *const _),
            ("AAsset_isAllocated", aasset::asset_is_alloc as *const _),
        ],
    );
}
// Find minecraftpe in dlpi
fn find_lib<'a>(target_name: &str) -> Option<plt_rs::LoadedLibrary<'a>> {
    let loaded_modules = plt_rs::collect_modules();
    loaded_modules
        .into_iter()
        .find(|lib| lib.name().ends_with(target_name))
}
// Backup of function ptr and its instructions
#[derive(Debug)]
struct MemBackup {
    backup_bytes: [u8; BACKUP_LEN],
    original_func_ptr: *mut u8,
}

unsafe impl Send for MemBackup {}
unsafe impl Sync for MemBackup {}
// Pointer to ResourcePackManager object
#[derive(Debug)]
pub struct ResourcePackManagerPtr(*mut c_void);
unsafe impl Send for ResourcePackManagerPtr {}
unsafe impl Sync for ResourcePackManagerPtr {}

static BACKUP: OnceLock<MemBackup> = OnceLock::new();
pub static PACKM_PTR: OnceLock<ResourcePackManagerPtr> = OnceLock::new();
pub static PACK_MANAGER: OnceLock<RpmLoadFn> = OnceLock::new();

#[inline(never)]
unsafe extern "C" fn hook_rpm_ctor(
    this: *mut c_void,
    unk1: usize,
    unk2: usize,
    needs_init: bool,
) -> *mut c_void {
    log::info!("rpm ctor called");
    let result = call_original(this, unk1, unk2, needs_init);
    // This will only run once
    if PACKM_PTR.get().is_none() {
        log::info!("RPM pointer has been obtained");
        PACKM_PTR.set(ResourcePackManagerPtr(this)).unwrap();
        PACK_MANAGER.set(get_load(this)).unwrap();
    }
    log::info!("hook exit");
    result
}
unsafe fn call_original(
    this: *mut c_void,
    unk1: usize,
    unk2: usize,
    needs_init: bool,
) -> *mut c_void {
    let backup = BACKUP.get().unwrap();
    // We unsetup this since its a one time thing
    // which also allows us to call the original fn
    unsafe { unsetup_hook(backup.original_func_ptr, backup.backup_bytes) };
    log::info!("RPMC hook is gone");
    // c is worse in this aspect change my mind
    let original = transmute::<
        *mut u8,
        unsafe extern "C" fn(*mut c_void, usize, usize, bool) -> *mut c_void,
    >(backup.original_func_ptr);
    let orig = original(this, unk1, unk2, needs_init);
    log::info!("called original function");
    orig
}
type RpmLoadFn = unsafe extern "C" fn(*mut c_void, *mut ResourceLocation, &mut CxxString) -> bool;
unsafe fn get_load(packm_ptr: *mut c_void) -> RpmLoadFn {
    // First dereference
    let vptr = *transmute::<*mut c_void, *mut *mut *const u8>(packm_ptr);
    // Now we offset by 2 to get load function and deref again
    // and then we transmute into a function pointer
    transmute::<*const u8, RpmLoadFn>(*vptr.offset(2))
}
fn dumb_callback() -> Option<(usize, usize)> {
    let mut range: (usize, usize) = (0, 0);
    unsafe {
        libc::dl_iterate_phdr(
            Some(callback),
            &mut range as *mut (usize, usize) as *mut libc::c_void,
        )
    };
    if range != (0, 0) {
        return Some(range);
    }
    None
}

unsafe extern "C" fn callback(
    phdr: *mut libc::dl_phdr_info,
    _size: usize,
    deeta: *mut libc::c_void,
) -> i32 {
    let Some(phdr) = phdr.as_ref() else {
        return 0;
    };
    let name = CStr::from_ptr(phdr.dlpi_name).to_string_lossy();
    if name.ends_with("libminecraftpe.so") {
        if phdr.dlpi_phnum == 0 {
            return 0;
        }
        // safe: we ensure that length is not zero, and trust the system to not go against us
        // and put a library with phdr set at some random out-of-bounds address
        let sections = slice::from_raw_parts(phdr.dlpi_phdr, phdr.dlpi_phnum as usize);
        const PF_X: u32 = 1 << 0;
        let Some(code_section) = sections
            .iter()
            .find(|phdr| phdr.p_type == PT_LOAD && (phdr.p_flags & PF_X) == 1)
        else {
            return 0;
        };
        let section_addr = phdr.dlpi_addr + code_section.p_vaddr;
        // unwrap is safe, we ensure we always give it a correct pointer
        let data_ref = (deeta as *mut (usize, usize)).as_mut().unwrap();
        *data_ref = (section_addr as usize, code_section.p_memsz as usize);
        return -1;
    }
    0
}
