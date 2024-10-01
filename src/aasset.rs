use cxx::{let_cxx_string, CxxString};
use libc::{off64_t, off_t};
use materialbin::{CompiledMaterialDefinition, MinecraftVersion};
use ndk_sys::{AAsset, AAssetManager};

use once_cell::sync::Lazy;
use scroll::Pread;
use std::{
    collections::HashMap,
    ffi::{CStr, CString, OsStr},
    io::{self, Cursor, Read, Seek},
    mem::transmute,
    ops::{Deref, DerefMut},
    os::unix::ffi::OsStrExt,
    path::Path,
    sync::{Mutex, OnceLock},
};

use crate::ResourceLocation;

// This makes me feel wrong... but all we will do is compare the pointer
// and the struct will be used in a mutex so i guess this is safe??
#[derive(PartialEq, Eq, Hash)]
struct AAssetPtr(*const ndk_sys::AAsset);
unsafe impl Send for AAssetPtr {}

// the assets we want to intercept access to
static WANTED_ASSETS: Lazy<Mutex<HashMap<AAssetPtr, Cursor<Vec<u8>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static MC_VERSION: OnceLock<MinecraftVersion> = OnceLock::new();

// This is unsafe because it calls stuff that can give us some nasty UB
// But we let ub happen because honestly this is a hook
fn get_latest_mcver(amanager: ndk::asset::AssetManager) -> Option<MinecraftVersion> {
    // This is kinda complicated but its simple
    let versions = [
        MinecraftVersion::V1_18_30,
        MinecraftVersion::V1_19_60,
        MinecraftVersion::V1_20_80,
        MinecraftVersion::V1_21_20,
    ];
    let android_prefix = "assets/resource_packs/vanilla_";
    let mut apk_version = None;
    // Since this does not stop after finding one, it will replace it with the
    // latest one if it exists
    for version in versions {
        let path = format!("{android_prefix}{version}/");
        if amanager.open_dir(&CString::new(path).unwrap()).is_some() {
            apk_version = Some(version);
        }
    }
    apk_version
}
pub(crate) unsafe fn asset_open(
    man: *mut AAssetManager,
    fname: *const libc::c_char,
    mode: libc::c_int,
) -> *mut ndk_sys::AAsset {
    log::info!("aaset open");
    // This is where ub can happen, but we are merely a hook.
    let aasset = unsafe { ndk_sys::AAssetManager_open(man, fname, mode) };
    let c_str = unsafe { CStr::from_ptr(fname) };
    let raw_cstr = c_str.to_bytes();
    if !raw_cstr.ends_with(b".material.bin") {
        return aasset;
    }
    log::info!("file is mtbin");
    let os_str = OsStr::from_bytes(raw_cstr);
    let c_path: &Path = Path::new(os_str);
    let Some(os_filename) = c_path.file_name() else {
        return aasset;
    };
    log::info!("str unwrap");
    let mtbin_path = "renderer/materials/".to_string() + &os_filename.to_string_lossy();
    log::info!("cxx thing");
    let_cxx_string!(cxx_str = mtbin_path);
    log::info!("created cxx str1");
    let_cxx_string!(cxx_out = "");
    log::info!("created cxx str2");
    let loadfn = match crate::PACK_MANAGER.get() {
        Some(ptr) => ptr,
        None => return aasset,
    };
    log::info!("thing seem to be going fine..?");
    let packm_ptr = crate::PACKM_PTR.get().unwrap();
    log::info!("we got packm ptr");
    let mut resource_loc = ResourceLocation::from_cxx_path(cxx_str.deref());
    log::info!("calling load fn");
    loadfn(
        packm_ptr.0,
        transmute(&mut resource_loc),
        transmute(cxx_out.as_mut().get_unchecked_mut()),
    );
    if cxx_out.is_empty() {
        log::warn!("cxx out is empty");
        return aasset;
    }
    let buffer = cxx_out.as_bytes().to_vec();
    let file = Cursor::new(match process_material(man, &buffer) {
        Some(updated) => updated,
        None => buffer,
    });
    let mut wanted_lock = WANTED_ASSETS.lock().unwrap();
    wanted_lock.insert(AAssetPtr(aasset), file);
    aasset
}
fn process_material(man: *mut AAssetManager, data: &[u8]) -> Option<Vec<u8>> {
    let mcver = MC_VERSION.get_or_init(|| {
        let pointer = std::ptr::NonNull::new(man).unwrap();
        let manager = unsafe { ndk::asset::AssetManager::from_ptr(pointer) };
        get_latest_mcver(manager).unwrap()
    });
    for version in materialbin::ALL_VERSIONS {
        let material: CompiledMaterialDefinition = match data.pread_with(0, version) {
            Ok(data) => data,
            Err(e) => {
                continue;
            }
        };

        let mut output = Vec::with_capacity(data.len());
        if let Err(e) = material.write(&mut output, *mcver) {
            return None;
        }
        return Some(output);
    }

    None
}
pub(crate) unsafe fn asset_seek64(
    aasset: *mut AAsset,
    off: off64_t,
    whence: libc::c_int,
) -> off64_t {
    log::info!("aasset seek64");
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_seek64(aasset, off, whence),
    };
    seek_facade(off, whence, file) as off64_t
}

pub(crate) unsafe fn asset_seek(aasset: *mut AAsset, off: off_t, whence: libc::c_int) -> off_t {
    log::info!("aasset seek32");
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_seek(aasset, off, whence),
    };
    // This code can be very deadly on large files,
    // but since NO replacement should surpass u32 max we should be fine...
    // i dont even think a mcpack can exceed that
    seek_facade(off.into(), whence, file) as off_t
}

pub(crate) unsafe fn asset_read(
    aasset: *mut AAsset,
    buf: *mut libc::c_void,
    count: libc::size_t,
) -> libc::c_int {
    log::info!("aasset read");
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_read(aasset, buf, count),
    };
    let mut rs_buffer: Vec<u8> = vec![0; count];
    let read_total = match file.read(&mut rs_buffer) {
        Ok(n) => n,
        Err(e) => {
            return -1 as libc::c_int;
        }
    };
    // try to make it as exact as possible
    rs_buffer.shrink_to_fit();
    // this is safe because we are gonna forget rs_buffer
    let data_ptr = rs_buffer.as_mut_ptr();
    // this should be safe since caller probably
    // has a array of this size
    let data_len = rs_buffer.len();
    // rs_buffer is now adopted
    std::mem::forget(rs_buffer);
    // fill c buffer
    unsafe {
        std::ptr::copy_nonoverlapping(data_ptr, buf as *mut u8, data_len);
    }
    read_total as libc::c_int
}

pub(crate) unsafe fn asset_length(aasset: *mut AAsset) -> off_t {
    let wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getLength(aasset),
    };
    file.get_ref().len() as off_t
}

pub(crate) unsafe fn asset_length64(aasset: *mut AAsset) -> off64_t {
    let wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getLength64(aasset),
    };
    file.get_ref().len() as off64_t
}

pub(crate) unsafe fn asset_remaining(aasset: *mut AAsset) -> off_t {
    let wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getRemainingLength(aasset),
    };
    (file.get_ref().len() - file.position() as usize) as off_t
}

pub(crate) unsafe fn asset_remaining64(aasset: *mut AAsset) -> off64_t {
    let wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getRemainingLength64(aasset),
    };
    (file.get_ref().len() - file.position() as usize) as off64_t
}

pub(crate) unsafe fn asset_close(aasset: *mut AAsset) {
    log::info!("aaset close");
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    if wanted_assets.remove(&AAssetPtr(aasset)).is_none() {
        ndk_sys::AAsset_close(aasset);
    }
}

// i hate this so much

pub(crate) unsafe fn asset_get_buffer(aasset: *mut AAsset) -> *const libc::c_void {
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getBuffer(aasset),
    };
    // aughhhhhhhhh
    // im scared shitless of this
    file.get_mut().as_mut_ptr().cast()
}

pub(crate) unsafe fn asset_fd_dummy(
    aasset: *mut AAsset,
    out_start: *mut off_t,
    out_len: *mut off_t,
) -> libc::c_int {
    let wanted_assets = WANTED_ASSETS.lock().unwrap();
    match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(_) => -1,
        None => ndk_sys::AAsset_openFileDescriptor(aasset, out_start, out_len),
    }
}

pub(crate) unsafe fn asset_fd_dummy64(
    aasset: *mut AAsset,
    out_start: *mut off64_t,
    out_len: *mut off64_t,
) -> libc::c_int {
    let wanted_assets = WANTED_ASSETS.lock().unwrap();
    match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(_) => -1,
        None => ndk_sys::AAsset_openFileDescriptor64(aasset, out_start, out_len),
    }
}

pub(crate) unsafe fn asset_is_alloc(aasset: *mut AAsset) -> libc::c_int {
    let wanted_assets = WANTED_ASSETS.lock().unwrap();
    match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(_) => false as libc::c_int,
        None => ndk_sys::AAsset_isAllocated(aasset),
    }
}

fn seek_facade(offset: i64, whence: libc::c_int, file: &mut Cursor<Vec<u8>>) -> i64 {
    let offset = match whence {
        libc::SEEK_SET => {
            //Lets check this so we dont mess up
            let u64_off = match u64::try_from(offset) {
                Ok(uoff) => uoff,
                Err(e) => {
                    return -1;
                }
            };
            io::SeekFrom::Start(u64_off)
        }
        libc::SEEK_CUR => io::SeekFrom::Current(offset),
        libc::SEEK_END => io::SeekFrom::End(offset),
        _ => return -1,
    };
    match file.seek(offset) {
        Ok(new_offset) => new_offset.try_into().unwrap(),
        Err(err) => -1,
    }
}
