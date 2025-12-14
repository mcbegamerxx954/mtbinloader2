//Explanation: Aasset is NOT thread-safe anyways so we will not try adding thread safety either
#![allow(static_mut_refs)]
use crate::{
    cpp_string::{ResourceLocation, StackString},
    LockResultExt,
};
use cxx::CxxString;
use libc::{c_char, c_int, c_void, off64_t, off_t, size_t};
use ndk::asset::AssetManager;
use ndk_sys::{AAsset, AAssetManager};
use once_cell::sync::Lazy;
use std::{
    cell::UnsafeCell,
    collections::HashMap,
    error::Error,
    ffi::{CStr, OsStr},
    io::{self, Cursor, Read, Seek, Write},
    ops::{Deref, DerefMut},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    pin::Pin,
    ptr,
    sync::{atomic::Ordering, LazyLock, Mutex},
};
static MC_FILELOADER: LazyLock<Mutex<FileLoader>> =
    LazyLock::new(|| Mutex::new(FileLoader { last_buffer: None }));
// This makes me feel wrong... but all we will do is compare the pointer
// and the struct will be used in a mutex so this is safe??
#[derive(PartialEq, Eq, Hash)]
struct AAssetPtr(*const ndk_sys::AAsset);
unsafe impl Send for AAssetPtr {}

// The assets we have registered to replace data about
static mut WANTED_ASSETS: Lazy<UnsafeCell<HashMap<AAssetPtr, Buffer>>> =
    Lazy::new(|| UnsafeCell::new(HashMap::new()));

macro_rules! folder_list {
    ($( apk: $apk_folder:literal -> pack: $pack_folder:expr),
        *,
    ) => {
        [
            $(($apk_folder, $pack_folder)),*,
        ]
    }
}
pub unsafe extern "C" fn open(
    man: *mut AAssetManager,
    fname: *const c_char,
    mode: c_int,
) -> *mut AAsset {
    // This is where UB can happen, but we are merely a hook.
    let aasset = unsafe { ndk_sys::AAssetManager_open(man, fname, mode) };
    let pointer = match std::ptr::NonNull::new(man) {
        Some(yay) => yay,
        None => {
            log::warn!("AssetManager is null?, preposterous, mc detection failed");
            return aasset;
        }
    };
    let manager = unsafe { ndk::asset::AssetManager::from_ptr(pointer) };
    let c_str = unsafe { CStr::from_ptr(fname) };
    let raw_cstr = c_str.to_bytes();
    let os_str = OsStr::from_bytes(raw_cstr);
    let c_path: &Path = Path::new(os_str);
    let mut sus = MC_FILELOADER.lock().ignore_poison();
    if let Some(yay) = sus.get_file(c_path, manager) {
        WANTED_ASSETS.get_mut().insert(AAssetPtr(aasset), yay);
    }
    aasset
}
macro_rules! handle_result {
    ($expr:expr) => {
        match $expr {
            Ok(val) => val,
            Err(e) => {
                log::error!("{e}");
                return -1;
            }
        }
    };
}
// This lint is not really applicable
#[allow(clippy::unused_io_amount)]
/// Join paths directly into a c++ string
fn opt_path_join(mut bytes: Pin<&mut CxxString>, paths: &[&Path]) {
    let total_len: usize = paths.iter().map(|p| p.as_os_str().len()).sum();
    bytes.as_mut().reserve(total_len);
    let mut writer = bytes;
    for path in paths {
        let osstr = path.as_os_str().as_bytes();
        writer
            .write(osstr)
            .expect("Error while writing path to stack path");
    }
}

pub unsafe extern "C" fn seek64(aasset: *mut AAsset, off: off64_t, whence: c_int) -> off64_t {
    let file = match WANTED_ASSETS.get_mut().get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_seek64(aasset, off, whence),
    };
    handle_result!(seek_facade(off, whence, file).try_into())
}

pub unsafe extern "C" fn seek(aasset: *mut AAsset, off: off_t, whence: c_int) -> off_t {
    let wanted_assets = WANTED_ASSETS.get_mut();
    let file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_seek(aasset, off, whence),
    };
    handle_result!(seek_facade(off.into(), whence, file).try_into())
}

pub unsafe extern "C" fn read(aasset: *mut AAsset, buf: *mut c_void, count: size_t) -> c_int {
    let wanted_assets = WANTED_ASSETS.get_mut();
    let file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_read(aasset, buf, count),
    };
    // Reuse buffer given by caller
    let rs_buffer = core::slice::from_raw_parts_mut(buf as *mut u8, count);
    let read_total = match (*file).read(rs_buffer) {
        Ok(n) => n,
        Err(e) => return e.to_c_result(),
    };
    handle_result!(read_total.try_into())
}

pub unsafe extern "C" fn len(aasset: *mut AAsset) -> off_t {
    let wanted_assets = WANTED_ASSETS.get_mut();
    let file = match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getLength(aasset),
    };
    handle_result!(file.get_ref().len().try_into())
}

pub unsafe extern "C" fn len64(aasset: *mut AAsset) -> off64_t {
    let wanted_assets = WANTED_ASSETS.get_mut();
    let file = match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getLength64(aasset),
    };
    handle_result!(file.get_ref().len().try_into())
}

pub unsafe extern "C" fn rem(aasset: *mut AAsset) -> off_t {
    let wanted_assets = WANTED_ASSETS.get_mut();
    let file = match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getRemainingLength(aasset),
    };
    handle_result!((file.get_ref().len() - file.position() as usize).try_into())
}

pub unsafe extern "C" fn rem64(aasset: *mut AAsset) -> off64_t {
    let wanted_assets = WANTED_ASSETS.get_mut();
    let file = match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getRemainingLength64(aasset),
    };
    handle_result!((file.get_ref().len() - file.position() as usize).try_into())
}

pub unsafe extern "C" fn close(aasset: *mut AAsset) {
    let wanted_assets = WANTED_ASSETS.get_mut();
    if let Some(buffer) = wanted_assets.remove(&AAssetPtr(aasset)) {
        MC_FILELOADER.lock().ignore_poison().last_buffer = Some(buffer);
    }
    ndk_sys::AAsset_close(aasset);
}

pub unsafe extern "C" fn get_buffer(aasset: *mut AAsset) -> *const c_void {
    let wanted_assets = WANTED_ASSETS.get_mut();
    let file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getBuffer(aasset),
    };
    // Let's hope this does not go boom boom
    file.get_ref().as_ptr().cast()
}

pub unsafe extern "C" fn fd_dummy(
    aasset: *mut AAsset,
    out_start: *mut off_t,
    out_len: *mut off_t,
) -> c_int {
    let wanted_assets = WANTED_ASSETS.get_mut();
    match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(_) => {
            log::error!("WE GOT BUSTED NOOO");
            -1
        }
        None => ndk_sys::AAsset_openFileDescriptor(aasset, out_start, out_len),
    }
}

pub unsafe extern "C" fn fd_dummy64(
    aasset: *mut AAsset,
    out_start: *mut off64_t,
    out_len: *mut off64_t,
) -> c_int {
    let wanted_assets = WANTED_ASSETS.get_mut();
    match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(_) => {
            log::error!("WE GOT BUSTED NOOO");
            -1
        }
        None => ndk_sys::AAsset_openFileDescriptor64(aasset, out_start, out_len),
    }
}

pub unsafe extern "C" fn is_alloc(aasset: *mut AAsset) -> c_int {
    let wanted_assets = WANTED_ASSETS.get_mut();
    match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(_) => false as c_int,
        None => ndk_sys::AAsset_isAllocated(aasset),
    }
}

fn seek_facade(offset: i64, whence: c_int, file: &mut Buffer) -> i64 {
    let offset = match whence {
        libc::SEEK_SET => {
            //Let's check this so we don't mess up
            let u64_off = match u64::try_from(offset) {
                Ok(uoff) => uoff,
                Err(e) => return e.to_c_result(),
            };
            io::SeekFrom::Start(u64_off)
        }
        libc::SEEK_CUR => io::SeekFrom::Current(offset),
        libc::SEEK_END => io::SeekFrom::End(offset),
        _ => {
            log::error!("Invalid seek whence");
            return -1;
        }
    };
    match file.seek(offset) {
        Ok(new_offset) => match new_offset.try_into() {
            Ok(int) => int,
            Err(err) => err.to_c_result(),
        },
        Err(err) => err.to_c_result(),
    }
}

enum BufferCursor {
    Vec(Cursor<Vec<u8>>),
    Cxx(Cursor<StackString>),
}
impl Read for BufferCursor {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Vec(v) => v.read(buf),
            Self::Cxx(cxx) => cxx.read(buf),
        }
    }
}
impl Seek for BufferCursor {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        match self {
            Self::Vec(v) => v.seek(pos),
            Self::Cxx(cxx) => cxx.seek(pos),
        }
    }
}
impl BufferCursor {
    fn position(&self) -> u64 {
        match self {
            Self::Vec(v) => v.position(),
            Self::Cxx(cxx) => cxx.position(),
        }
    }
    fn get_ref(&self) -> &[u8] {
        match self {
            Self::Vec(v) => v.get_ref(),
            Self::Cxx(cxx) => cxx.get_ref().as_ref(),
        }
    }
}

struct FileLoader {
    last_buffer: Option<Buffer>,
}
impl FileLoader {
    fn get_file(&mut self, path: &Path, manager: AssetManager) -> Option<Buffer> {
        let stripped = path.strip_prefix("assets/").unwrap_or(path);
        if let Some(mut cache) = self.last_buffer.take_if(|c| c.name == path) {
            log::info!("Cache hit!: {:#?}", path);
            cache
                .rewind()
                .expect("Unable to rewind in a memory buffer?, impossible");
            return Some(cache);
        }
        let replacement_list = folder_list! {
            apk: "gui/dist/hbui/" -> pack: "hbui/",
            apk: "skin_packs/persona/" -> pack: "persona/",
            apk: "renderer/" -> pack: "renderer/",
            apk: "resource_packs/vanilla/cameras/" -> pack: "vanilla_cameras/",
        };
        for replacement in replacement_list {
            // Remove the prefix we want to change
            if let Ok(file) = stripped.strip_prefix(replacement.0) {
                let mut cxx_storage = StackString::new();
                let mut cxx_ptr = unsafe { cxx_storage.init("") };
                let Some(loadfn) = crate::RPM_LOAD.get() else {
                    log::warn!("ResourcePackManager fn is not ready yet?");
                    return None;
                };
                let mut resource_loc = ResourceLocation::new();
                let mut cpppath = ResourceLocation::get_path(&mut resource_loc);
                opt_path_join(cpppath.as_mut(), &[Path::new(replacement.1), file]);
                let packm_ptr = crate::PACKM_OBJ.load(Ordering::Acquire);
                if packm_ptr.is_null() {
                    log::error!("ResourcePackManager ptr is null");
                    return None;
                }
                unsafe {
                    loadfn(packm_ptr, resource_loc, cxx_ptr.as_mut());
                }
                if cxx_ptr.is_empty() {
                    log::info!("Cannot find file: {}", cpppath.as_ref());
                    return None;
                }
                log::info!("Loaded ResourcePack file: {}", cpppath.as_ref());
                let buffer = if file
                    .as_os_str()
                    .as_encoded_bytes()
                    .ends_with(b".material.bin")
                {
                    match crate::autofixer::process_material(manager, cxx_ptr.as_bytes()) {
                        Some(updated) => BufferCursor::Vec(Cursor::new(updated)),
                        None => BufferCursor::Cxx(Cursor::new(cxx_storage)),
                    }
                } else {
                    BufferCursor::Cxx(Cursor::new(cxx_storage))
                };
                let cache = Buffer::new(path.to_path_buf(), buffer);
                // ResourceLocation gets dropped (also cxx_storage if its not needed)
                return Some(cache);
            }
        }
        None
    }
}
struct Buffer {
    name: PathBuf,
    object: BufferCursor,
}
impl Buffer {
    fn new(name: PathBuf, object: BufferCursor) -> Self {
        Self { name, object }
    }
}
impl Deref for Buffer {
    type Target = BufferCursor;
    fn deref(&self) -> &Self::Target {
        &self.object
    }
}
impl DerefMut for Buffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.object
    }
}
trait AsCResult<T> {
    fn to_c_result(&self) -> T;
}
impl<P, T: Error> AsCResult<*mut P> for T {
    fn to_c_result(&self) -> *mut P {
        log::error!("Error: {self}");
        ptr::null_mut()
    }
}

impl<P, T: Error> AsCResult<*const P> for T {
    fn to_c_result(&self) -> *const P {
        log::error!("Error: {self}");
        ptr::null_mut()
    }
}

impl<T: Error> AsCResult<c_int> for T {
    fn to_c_result(&self) -> c_int {
        log::error!("Error: {self}");
        -1
    }
}

impl<T: Error> AsCResult<off_t> for T {
    fn to_c_result(&self) -> off_t {
        log::error!("Error: {self}");
        -1
    }
}
