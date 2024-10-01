use std::{
    borrow::BorrowMut,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
mod aasset;
mod hooking;
mod plthook;
use core::mem::transmute;
use cxx::CxxString;
use hooking::{setup_hook, unsetup_hook};
use libc::c_void;
use lightningscanner::Scanner;
use plt_rs::DynamicLibrary;

use crate::plthook::replace_plt_functions;

#[repr(C)]
pub struct ResourceLocation<'a> {
    file_system: i64,
    path: &'a CxxString,
    path_hash: u64,
    full_hash: u64,
}
impl<'a> ResourceLocation<'a> {
    pub fn from_cxx_path(path: &'a CxxString) -> Self {
        Self {
            file_system: 0,
            path,
            path_hash: 0,
            full_hash: 0,
        }
    }
}
pub fn setup_logging() {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Trace),
    );
}
#[ctor::ctor]
fn main() {
    // let what = plt_rs::collect_modules();
    // let mcpe = what
    //     .into_iter()
    //     .find(|m| m.name().contains("libminecraftpe.so"))
    //     .expect("no lib found");
    // let code_part = mcpe
    //     .program_headers()
    //     .find(|w| w.header_type() == PT_LOAD && w.flags() & PF_X == 1)
    //     .expect("no load header found");
    // let from = code_part.program_addr();
    log::info!("hiii");
    setup_logging();
    let mut path = Path::new("/proc/self/").to_path_buf();
    let procmaps = procmaps::Mappings::from_path(&mut path).unwrap();
    let mcmap = procmaps
        .iter()
        .find(|map| {
            if let procmaps::Path::MappedFile(filename) = &map.pathname {
                if filename.contains("libminecraftpe.so") {
                    return true;
                }
            }
            false
        })
        .unwrap();
    // Pattern taken from materialbinloader
    let scanner = Scanner::new("FF 03 03 D1 FD 7B 07 A9 FD C3 01 91 F9 43 00 F9 F8 5F 09 A9 F6 57 0A A9 F4 4F 0B A9 59 D0 3B D5 F6 03 03 2A 28 17 40 F9 F5 03 02 AA F3 03 00 AA A8 83 1F F8 28 10 40 F9");
    let addr = unsafe { scanner.find(None, mcmap.base as *const u8, mcmap.size_of_mapping()) };
    let addr = addr.get_addr();
    if addr.is_null() {
        panic!("cannot find signature");
    }
    log::info!("hooking rpm");
    let result = unsafe { setup_hook(addr as *mut _, hResourcePackManager_ctor as *mut _) };
    BACKUP
        .set(MemBackup {
            backup_bytes: result.expect("hooking failed.."),
            original_func_ptr: addr as *mut _,
        })
        .unwrap();
    log::info!("hooking aasset, done");
    hook_aaset();
}
// Setup asset hooks
pub fn hook_aaset() {
    const LIBNAME: &str = "libminecraftpe";
    let lib_entry = match find_lib(LIBNAME) {
        Some(lib) => lib,
        None => panic!(),
    };
    let dyn_lib = match DynamicLibrary::initialize(lib_entry) {
        Ok(lib) => lib,
        Err(e) => panic!(),
    };
    replace_plt_functions(
        &dyn_lib,
        &[
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

fn find_lib<'a>(target_name: &str) -> Option<plt_rs::LoadedLibrary<'a>> {
    let loaded_modules = plt_rs::collect_modules();
    loaded_modules
        .into_iter()
        .find(|lib| lib.name().contains(target_name))
}
#[derive(Debug)]
struct MemBackup {
    backup_bytes: [u8; 16],
    original_func_ptr: *mut libc::c_void,
}
// This is very single use so we dont care
unsafe impl Send for MemBackup {}
unsafe impl Sync for MemBackup {}

struct PackManagerPtr(*mut c_void);
unsafe impl Send for PackManagerPtr {}
unsafe impl Sync for PackManagerPtr {}

static BACKUP: OnceLock<MemBackup> = OnceLock::new();
pub static PACKM_PTR: OnceLock<PackManagerPtr> = OnceLock::new();
pub static PACK_MANAGER: OnceLock<
    unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> bool,
> = OnceLock::new();

#[inline(never)]
unsafe extern "C" fn hResourcePackManager_ctor(
    this: *mut libc::c_void,
    unk1: usize,
    unk2: usize,
    needs_init: bool,
) -> *mut libc::c_void {
    log::info!("rpm ctor called");
    let result = call_original(this, unk1, unk2, needs_init);
    // This will only run once
    if PACKM_PTR.get().is_none() {
        log::warn!("packm ptr is setup");
        PACKM_PTR.set(PackManagerPtr(this));
        PACK_MANAGER.set(get_load(this));
    } else if PACKM_PTR.get().is_none() {
        log::info!("rehooking...");
        let back = BACKUP.get().unwrap();
        setup_hook(back.original_func_ptr, hResourcePackManager_ctor as *mut _);
    }
    log::info!("hook exit");
    result
}
unsafe fn call_original(
    this: *mut libc::c_void,
    unk1: usize,
    unk2: usize,
    needs_init: bool,
) -> *mut libc::c_void {
    let backup = BACKUP.get().unwrap();
    // We unsetup this since its a one time thing
    // which also allows us to call the original fn
    unsafe { unsetup_hook(backup.original_func_ptr, backup.backup_bytes) };
    log::info!("undone rpm hook");
    // c is worse in this aspect change my mind
    let original = transmute::<
        *mut libc::c_void,
        unsafe extern "C" fn(*mut libc::c_void, usize, usize, bool) -> *mut libc::c_void,
    >(backup.original_func_ptr);
    log::info!("talk to mc time");
    let orig = original(this, unk1, unk2, needs_init);
    log::info!("worked yippie");
    orig
}
unsafe fn get_load(
    packm_ptr: *mut libc::c_void,
) -> unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> bool {
    let mut vptr = *transmute::<*mut c_void, *mut *mut unsafe extern "C" fn()>(packm_ptr);
    let mut load = core::mem::transmute::<
        unsafe extern "C" fn(),
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> bool,
    >(*vptr.offset(2));
    load
}
