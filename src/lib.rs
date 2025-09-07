//#[deny(indexing_slicing)]
mod cpp_string;
use std::{
    fs,
    pin::Pin,
    ptr::null_mut,
    sync::{atomic::AtomicPtr, OnceLock},
};
mod aasset;
mod jniopts;
mod plthook;
use crate::plthook::replace_plt_functions;
use bhook::hook_fn;
use bstr::ByteSlice;
use cpp_string::ResourceLocation;
//use bstr::ByteSlice;
use atoi::FromRadix16;
use core::mem::transmute;
use cxx::CxxString;
use libc::c_void;
use plt_rs::DynamicLibrary;
use tinypatscan::Pattern;
#[cfg(target_arch = "aarch64")]
const RPMC_PATTERNS: [Pattern<80>; 3] = [
    // V1.21.100
    Pattern::from_str("FF C3 02 D1 FD 7B 06 A9 FD 83 01 91 F9 3B 00 F9 F8 5F 08 A9 F6 57 09 A9 F4 4F 0A A9 59 D0 3B D5 F6 03 03 2A 28 17 40 F9 F5 03 02 AA F3 03 00 AA A8 83 1F F8 28 10 40 F9"),
    // older than V1.21.100
    Pattern::from_str("FF 03 03 D1 FD 7B 07 A9 FD C3 01 91 F9 43 00 F9 F8 5F 09 A9 F6 57 0A A9 F4 4F 0B A9 59 D0 3B D5 F6 03 03 2A 28 17 40 F9 F5 03 02 AA F3 03 00 AA A8 83 1F F8 28 10 40 F9"),
    Pattern::from_str("FF 83 02 D1 FD 7B 06 A9 FD 83 01 91 F8 5F 07 A9 F6 57 08 A9 F4 4F 09 A9 58 D0 3B D5 F6 03 03 2A 08 17 40 F9 F5 03 02 AA F3 03 00 AA A8 83 1F F8 28 10 40 F9 28 01 00 B4"),
];
#[cfg(target_arch = "arm")]
const RPMC_PATTERNS: [Pattern<80>; 1] = [Pattern::from_str(
    // V1.21.100
    "F0 B5 03 AF 2D E9 00 ?? ?? B0 ?? 46 ?? 48 98 46 92 46 78 44 00 68 00 68 ?? 90 08 69",
)];
#[cfg(target_arch = "x86_64")]
const RPMC_PATTERNS: [Pattern<80>; 2] = [
    Pattern::from_str("55 41 57 41 56 41 55 41 54 53 48 83 EC ? 41 89 CF 49 89 D6 48 89 FB 64 48 8B 04 25 28 00 00 00 48 89 44 24 ? 48 8B 7E"),
    Pattern::from_str("55 41 57 41 56 53 48 83 EC ? 41 89 CF 49 89 D6 48 89 FB 64 48 8B 04 25 28 00 00 00 48 89 44 24 ? 48 8B 7E"),
];

// Just setup the logger so we see those logcats
pub fn setup_logging() {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Trace),
    );
}
#[ctor::ctor]
fn safe_setup() {
    setup_logging();
    std::panic::set_hook(Box::new(move |panic_info| {
        log::error!("Thread crashed: {}", panic_info);
    }));
    // Let it crash and burn if anything happens
    main();
}
fn main() {
    log::info!("Starting, mbl2 version v0.1.10-beta");
    let mcmap = find_minecraft_library_manually()
        .expect("Cannot find libminecraftpe.so in memory maps - device not supported");
    let addr = find_signatures(&RPMC_PATTERNS, mcmap).expect("No signature was found");
    log::info!("Hooking ResourcePackManager constructor");
    unsafe {
        rpm_ctor::hook_address(addr as *mut u8);
    };
    log::info!("Hooking AssetManager functions");
    hook_aaset();
}
// A very minimal map range
#[derive(Debug)]
struct SimpleMapRange {
    start: usize,
    size: usize,
}

impl SimpleMapRange {
    /// Get the address where this range starts
    const fn start(&self) -> usize {
        self.start
    }

    /// Get the address where this range ends
    const fn size(&self) -> usize {
        self.size
    }
}

fn find_minecraft_library_manually() -> Result<SimpleMapRange, Box<dyn std::error::Error>> {
    let contents = fs::read("/proc/self/maps")?;
    for line in contents.lines() {
        if line.trim_ascii().is_empty() {
            continue;
        }
        // Not too pretty but this method prevents crashes
        let Some((addr_start, addr_end)) = parse_range(line) else {
            continue;
        };
        let start = usize::from_radix_16(addr_start).0;
        let end = usize::from_radix_16(addr_end).0;
        log::info!("Found libminecraftpe.so at: {:x}-{:x}", start, end);
        return Ok(SimpleMapRange {
            start,
            size: end - start,
        });
    }

    Err("libminecraftpe.so not found in memory maps".into())
}
/// Separated into function due to option spam
fn parse_range(buf: &[u8]) -> Option<(&[u8], &[u8])> {
    let mut line = buf.split(|v| v.is_ascii_whitespace());
    let addr_range = line.next()?;
    let perms = line.next()?;
    let pathname = line.next_back()?;
    if perms.contains(&b'x') && pathname.ends_with(b"libminecraftpe.so") {
        return addr_range.split_once_str(b"-");
    }
    None
}

fn find_signatures(signatures: &[Pattern<80>], range: SimpleMapRange) -> Option<*const u8> {
    for sig in signatures {
        let libbytes =
            unsafe { core::slice::from_raw_parts(range.start() as *const u8, range.size()) };

        let addr = if cfg!(target_arch = "arm") {
            sig.search(libbytes)
        } else {
            sig.simd_search(libbytes)
        };
        let addr = match addr {
            Some(val) => unsafe { libbytes.as_ptr().byte_add(val) },
            None => {
                log::error!("Cannot find signature");
                continue;
            }
        };
        #[cfg(target_arch = "arm")]
        let addr = unsafe { addr.offset(1) };
        return Some(addr as *const u8);
    }
    None
}

macro_rules! cast_array {
    ($($func_name:literal -> $hook:expr),
        *,
    ) => {
        [
            $(($func_name, $hook as *const u8)),*,
        ]
    }
}
/// Set up the asset manager hooks so we control APK file access
pub fn hook_aaset() {
    let lib_entry = find_lib("libminecraftpe").expect("Cannot find minecraftpe");
    let dyn_lib = DynamicLibrary::initialize(lib_entry).expect("Failed to find mc info");
    // Functions of aasset
    let asset_fn_list = cast_array! {
        "AAssetManager_open" -> aasset::open,
        "AAsset_read" -> aasset::read,
        "AAsset_close" -> aasset::close,
        "AAsset_seek" -> aasset::seek,
        "AAsset_seek64" -> aasset::seek64,
        "AAsset_getLength" -> aasset::len,
        "AAsset_getLength64" -> aasset::len64,
        "AAsset_getRemainingLength" -> aasset::rem,
        "AAsset_getRemainingLength64" -> aasset::rem64,
        "AAsset_openFileDescriptor" -> aasset::fd_dummy,
        "AAsset_openFileDescriptor64" -> aasset::fd_dummy64,
        "AAsset_getBuffer" -> aasset::get_buffer,
        "AAsset_isAllocated" -> aasset::is_alloc,
    };
    //The actual work
    replace_plt_functions(&dyn_lib, asset_fn_list);
}
/// Find some library's PLT
fn find_lib<'a>(target_name: &str) -> Option<plt_rs::LoadedLibrary<'a>> {
    let loaded_modules = plt_rs::collect_modules();
    loaded_modules
        .into_iter()
        .find(|lib| lib.name().contains(target_name))
}
// A resource pack manager object
pub static PACKM_OBJ: AtomicPtr<libc::c_void> = AtomicPtr::new(null_mut());
// The resource pack manager load function
pub static RPM_LOAD: OnceLock<RpmLoadFn> = OnceLock::new();

hook_fn! {
    fn rpm_ctor(this: *mut libc::c_void,unk1: usize,unk2: usize,needs_init: bool) -> *mut libc::c_void = {
        use std::sync::atomic::Ordering;
        log::info!("rpm ctor called");
        let result = call_original(this, unk1, unk2, needs_init);
        log::info!("RPM pointer has been obtained");
        crate::PACKM_OBJ.store(this, Ordering::Release);
        crate::RPM_LOAD.set(crate::get_load(this)).expect("Load function is only hooked once");
        // Not doing this just adds overhead
        self_disable();
        log::info!("hook exit");
        result
    }
}

type RpmLoadFn = unsafe extern "C" fn(*mut c_void, ResourceLocation, Pin<&mut CxxString>) -> bool;
/// Ahh c++, truly the language of all time
unsafe fn get_load(packm_ptr: *mut c_void) -> RpmLoadFn {
    let vptr = *transmute::<*mut c_void, *mut *mut *const u8>(packm_ptr);
    transmute::<*const u8, RpmLoadFn>(*vptr.offset(2))
}
