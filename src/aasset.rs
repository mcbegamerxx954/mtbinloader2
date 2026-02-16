//Explanation: Aasset is NOT thread-safe anyways so we will not try adding thread safety either
#![allow(static_mut_refs)]
use crate::{
    loader::{Buffer, FileLoader},
    LockResultExt,
};
use libc::{c_char, c_int, c_void, off64_t, off_t, size_t};
use ndk_sys::{AAsset, AAssetManager};
use std::{
    cell::UnsafeCell,
    collections::HashMap,
    ffi::{CStr, OsStr},
    io::{self, Read, Seek},
    os::unix::ffi::OsStrExt,
    path::Path,
    //    ptr,
    sync::{LazyLock, Mutex},
};
static MC_FILELOADER: LazyLock<Mutex<FileLoader>> = LazyLock::new(|| Mutex::new(FileLoader::new()));
// This makes me feel wrong... but all we will do is compare the pointer
// and the struct will be used in a mutex so this is safe??
#[derive(PartialEq, Eq, Hash)]
struct AAssetPtr(*const ndk_sys::AAsset);
unsafe impl Send for AAssetPtr {}

// The assets we have registered to replace data about
static mut WANTED_ASSETS: LazyLock<UnsafeCell<HashMap<AAssetPtr, Buffer>>> =
    LazyLock::new(|| UnsafeCell::new(HashMap::new()));

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

pub unsafe extern "C" fn seek64(aasset: *mut AAsset, off: off64_t, whence: c_int) -> off64_t {
    let Some(file) = WANTED_ASSETS.get_mut().get_mut(&AAssetPtr(aasset)) else {
        return ndk_sys::AAsset_seek64(aasset, off, whence);
    };
    handle_result!(seek_facade(off, whence, file).try_into())
}

pub unsafe extern "C" fn seek(aasset: *mut AAsset, off: off_t, whence: c_int) -> off_t {
    let wanted_assets = WANTED_ASSETS.get_mut();
    let Some(file) = wanted_assets.get_mut(&AAssetPtr(aasset)) else {
        return ndk_sys::AAsset_seek(aasset, off, whence);
    };
    handle_result!(seek_facade(off.into(), whence, file).try_into())
}

pub unsafe extern "C" fn read(aasset: *mut AAsset, buf: *mut c_void, count: size_t) -> c_int {
    let wanted_assets = WANTED_ASSETS.get_mut();
    let Some(file) = wanted_assets.get_mut(&AAssetPtr(aasset)) else {
        return ndk_sys::AAsset_read(aasset, buf, count);
    };
    // Reuse buffer given by caller
    let rs_buffer = core::slice::from_raw_parts_mut(buf as *mut u8, count);
    let read_total = handle_result!((*file).read(rs_buffer));
    handle_result!(read_total.try_into())
}

pub unsafe extern "C" fn len(aasset: *mut AAsset) -> off_t {
    let wanted_assets = WANTED_ASSETS.get_mut();
    let Some(file) = wanted_assets.get(&AAssetPtr(aasset)) else {
        return ndk_sys::AAsset_getLength(aasset);
    };
    handle_result!(file.get_ref().len().try_into())
}

pub unsafe extern "C" fn len64(aasset: *mut AAsset) -> off64_t {
    let wanted_assets = WANTED_ASSETS.get_mut();
    let Some(file) = wanted_assets.get(&AAssetPtr(aasset)) else {
        return ndk_sys::AAsset_getLength64(aasset);
    };
    handle_result!(file.get_ref().len().try_into())
}

pub unsafe extern "C" fn rem(aasset: *mut AAsset) -> off_t {
    let wanted_assets = WANTED_ASSETS.get_mut();
    let Some(file) = wanted_assets.get(&AAssetPtr(aasset)) else {
        return ndk_sys::AAsset_getRemainingLength(aasset);
    };
    handle_result!((file.get_ref().len() - file.position() as usize).try_into())
}

pub unsafe extern "C" fn rem64(aasset: *mut AAsset) -> off64_t {
    let wanted_assets = WANTED_ASSETS.get_mut();
    let Some(file) = wanted_assets.get(&AAssetPtr(aasset)) else {
        return ndk_sys::AAsset_getRemainingLength64(aasset);
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
    let Some(file) = wanted_assets.get_mut(&AAssetPtr(aasset)) else {
        return ndk_sys::AAsset_getBuffer(aasset);
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
            let u64_off = handle_result!(u64::try_from(offset));
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
        Ok(new_offset) => handle_result!(new_offset.try_into()),
        Err(err) => {
            log::error!("seek Error: {err}");
            return -1;
        }
    }
}
