use crate::ResourceLocation;
use libc::{off64_t, off_t};
use materialbin::{CompiledMaterialDefinition, MinecraftVersion};
use ndk::asset::Asset;
use ndk_sys::{AAsset, AAssetManager};
use once_cell::sync::Lazy;
use scroll::Pread;
use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::{CStr, CString, OsStr},
    io::{self, Cursor, Read, Seek},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

// This makes me feel wrong... but all we will do is compare the pointer
// and the struct will be used in a mutex so i guess this is safe??
#[derive(PartialEq, Eq, Hash)]
struct AAssetPtr(*const ndk_sys::AAsset);
unsafe impl Send for AAssetPtr {}

// The minecraft version we will use to port shaders to
static MC_VERSION: OnceLock<Option<MinecraftVersion>> = OnceLock::new();

// The assets we have registrered to remplace data about
static WANTED_ASSETS: Lazy<Mutex<HashMap<AAssetPtr, Cursor<Vec<u8>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// Im very sorry but its just that AssetManager is so shitty to work with
// i cant handle how randomly it breaks
fn get_current_mcver(man: ndk::asset::AssetManager) -> Option<MinecraftVersion> {
    let mut file = match get_uitext(man) {
        Some(asset) => asset,
        None => {
            log::error!("Shader fixing is disabled as no mc version was found");
            return None;
        }
    };
    let mut buf = Vec::with_capacity(file.length());
    if let Err(e) = file.read_to_end(&mut buf) {
        log::error!("Something is wrong with AssetManager, mc detection failed: {e}");
        return None;
    };
    for version in materialbin::ALL_VERSIONS {
        if buf
            .pread_with::<CompiledMaterialDefinition>(0, version)
            .is_ok()
        {
            log::info!("Mc version is {version}");
            return Some(version);
        };
    }
    None
}

// Try to open UIText.material.bin to guess mc shader version
fn get_uitext(man: ndk::asset::AssetManager) -> Option<Asset> {
    const NEW: &CStr = c"assets/renderer/materials/UIText.material.bin";
    const OLD: &CStr = c"renderer/materials/UIText.material.bin";
    for path in [NEW, OLD] {
        if let Some(asset) = man.open(path) {
            return Some(asset);
        }
    }
    None
}
macro_rules! folder_list {
    ($( apk: $apk_folder:literal -> pack: $pack_folder:expr),
        *,
    ) => {
        [
            $(($apk_folder, $pack_folder)),*,
        ]
    }
}
pub(crate) unsafe fn open(
    man: *mut AAssetManager,
    fname: *const libc::c_char,
    mode: libc::c_int,
) -> *mut ndk_sys::AAsset {
    // This is where ub can happen, but we are merely a hook.
    let aasset = unsafe { ndk_sys::AAssetManager_open(man, fname, mode) };
    let c_str = unsafe { CStr::from_ptr(fname) };
    let raw_cstr = c_str.to_bytes();
    let os_str = OsStr::from_bytes(raw_cstr);
    let c_path: &Path = Path::new(os_str);
    // Extract filename
    let Some(os_filename) = c_path.file_name() else {
        log::warn!("Path had no filename: {c_path:?}");
        return aasset;
    };
    // This is meant to strip the new "asset" folder path so we can be compatible with other versions
    let stripped = match c_path.strip_prefix("assets/") {
        Ok(yay) => yay,
        Err(e) => c_path,
    };
    // Folder paths to replace and with what
    let replacement_list = folder_list! {
        apk: "gui/dist/hbui/" -> pack: "hbui/",
        apk: "skin_packs/persona/" -> pack: "custom_persona/",
        apk: "renderer/" -> pack: "renderer/",
        apk: "resource_packs/vanilla/cameras/" -> pack: "vanilla_cameras/",
    };
    for replacement in replacement_list {
        // Remove the prefix we want to change
        if let Ok(file) = stripped.strip_prefix(replacement.0) {
            cxx::let_cxx_string!(cxx_out = "");
            let loadfn = match crate::PACK_MANAGER.get() {
                Some(ptr) => ptr,
                None => {
                    log::warn!("ResourcePackManager fn is not ready yet?");
                    return aasset;
                }
            };
            let mut arraybuf = [0; 128];
            let file_path = opt_path_join(&mut arraybuf, &[Path::new(replacement.1), file]);
            let packm_ptr = crate::PACKM_PTR.get().unwrap();
            // We allocate here but its alright
            let fpath = match CString::new(file_path.as_os_str().as_encoded_bytes()) {
                Ok(yay) => yay,
                Err(e) => {
                    log::warn!("Something went very very wrong (Path to CString): {e}");
                    return aasset;
                }
            };
            let resource_loc = ResourceLocation::from_str(&fpath);
            log::info!("loading rpck file: {:?}", &fpath);
            if packm_ptr.0.is_null() {
                log::error!("ResourcePackManager ptr is null");
                return aasset;
            }
            loadfn(packm_ptr.0, resource_loc, cxx_out.as_mut());
            // Free resource location
            ResourceLocation::free(resource_loc);
            if cxx_out.is_empty() {
                log::info!("File was not found");
                return aasset;
            }
            let buffer = if os_filename.as_encoded_bytes().ends_with(b".material.bin") {
                match process_material(man, cxx_out.as_bytes()) {
                    Some(updated) => updated,
                    None => cxx_out.as_bytes().to_vec(),
                }
            } else {
                cxx_out.as_bytes().to_vec()
            };
            let mut wanted_lock = WANTED_ASSETS.lock().unwrap();
            wanted_lock.insert(AAssetPtr(aasset), Cursor::new(buffer));
            // we do not clwan cxx string because cxx ceate does that for us
            return aasset;
        }
    }
    return aasset;
}
/// Join paths without allocating if possible, or
/// if the joined path does not fit the buffer then just
/// allocate instead
fn opt_path_join<'a>(bytes: &'a mut [u8; 128], paths: &[&Path]) -> Cow<'a, Path> {
    let total_len: usize = paths.iter().map(|p| p.as_os_str().len()).sum();
    if total_len > bytes.len() {
        // panic!("fuck");
        let mut pathbuf = PathBuf::new();
        for path in paths {
            pathbuf.push(path);
        }
        return Cow::Owned(pathbuf);
    }

    let mut len = 0;
    for path in paths {
        let osstr = path.as_os_str().as_bytes();
        (bytes[len..len + osstr.len()]).copy_from_slice(osstr);
        len += osstr.len();
    }
    let osstr = OsStr::from_bytes(&bytes[..len]);
    Cow::Borrowed(Path::new(osstr))
}
fn process_material(man: *mut AAssetManager, data: &[u8]) -> Option<Vec<u8>> {
    let mcver = MC_VERSION.get_or_init(|| {
        let pointer = match std::ptr::NonNull::new(man) {
            Some(yay) => yay,
            None => {
                log::warn!("AssetManager is null?, preposterous, mc detection failed");
                return None;
            }
        };
        let manager = unsafe { ndk::asset::AssetManager::from_ptr(pointer) };
        get_current_mcver(manager)
    });
    // just ignore if no mc version was found
    let mcver = (*mcver)?;
    for version in materialbin::ALL_VERSIONS {
        let material: CompiledMaterialDefinition = match data.pread_with(0, version) {
            Ok(data) => data,
            Err(e) => {
                log::trace!("[version] Parsing failed: {e}");
                continue;
            }
        };
        // Prevent some work
        if version == mcver {
            return None;
        }
        let mut output = Vec::with_capacity(data.len());
        if let Err(e) = material.write(&mut output, mcver) {
            log::trace!("[version] Write error: {e}");
            return None;
        }
        return Some(output);
    }

    None
}
pub(crate) unsafe fn seek64(aasset: *mut AAsset, off: off64_t, whence: libc::c_int) -> off64_t {
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_seek64(aasset, off, whence),
    };
    seek_facade(off, whence, file) as off64_t
}

pub(crate) unsafe fn seek(aasset: *mut AAsset, off: off_t, whence: libc::c_int) -> off_t {
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

pub(crate) unsafe fn read(
    aasset: *mut AAsset,
    buf: *mut libc::c_void,
    count: libc::size_t,
) -> libc::c_int {
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_read(aasset, buf, count),
    };
    // Reuse buffer given by caller
    let rs_buffer = core::slice::from_raw_parts_mut(buf as *mut u8, count);
    let read_total = match file.read(rs_buffer) {
        Ok(n) => n,
        Err(e) => {
            log::warn!("failed fake aaset read: {e}");
            return -1 as libc::c_int;
        }
    };
    read_total as libc::c_int
}

pub(crate) unsafe fn len(aasset: *mut AAsset) -> off_t {
    let wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getLength(aasset),
    };
    file.get_ref().len() as off_t
}

pub(crate) unsafe fn len64(aasset: *mut AAsset) -> off64_t {
    let wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getLength64(aasset),
    };
    file.get_ref().len() as off64_t
}

pub(crate) unsafe fn rem(aasset: *mut AAsset) -> off_t {
    let wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getRemainingLength(aasset),
    };
    (file.get_ref().len() - file.position() as usize) as off_t
}

pub(crate) unsafe fn rem64(aasset: *mut AAsset) -> off64_t {
    let wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getRemainingLength64(aasset),
    };
    (file.get_ref().len() - file.position() as usize) as off64_t
}

pub(crate) unsafe fn close(aasset: *mut AAsset) {
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    if wanted_assets.remove(&AAssetPtr(aasset)).is_none() {
        ndk_sys::AAsset_close(aasset);
    }
}

pub(crate) unsafe fn get_buffer(aasset: *mut AAsset) -> *const libc::c_void {
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getBuffer(aasset),
    };
    // Lets hope this does not go boom boom
    file.get_mut().as_mut_ptr().cast()
}

pub(crate) unsafe fn fd_dummy(
    aasset: *mut AAsset,
    out_start: *mut off_t,
    out_len: *mut off_t,
) -> libc::c_int {
    let wanted_assets = WANTED_ASSETS.lock().unwrap();
    match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(_) => {
            log::error!("WE GOT BUSTED NOOO");
            -1
        }
        None => ndk_sys::AAsset_openFileDescriptor(aasset, out_start, out_len),
    }
}

pub(crate) unsafe fn fd_dummy64(
    aasset: *mut AAsset,
    out_start: *mut off64_t,
    out_len: *mut off64_t,
) -> libc::c_int {
    let wanted_assets = WANTED_ASSETS.lock().unwrap();
    match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(_) => {
            log::error!("WE GOT BUSTED NOOO");
            -1
        }
        None => ndk_sys::AAsset_openFileDescriptor64(aasset, out_start, out_len),
    }
}

pub(crate) unsafe fn is_alloc(aasset: *mut AAsset) -> libc::c_int {
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
                    log::error!("signed ({offset}) to unsigned failed: {e}");
                    return -1;
                }
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
            Err(err) => {
                log::error!("u64 ({new_offset}) to i64 failed: {err}");
                -1
            }
        },
        Err(err) => {
            log::error!("aasset seek failed: {err}");
            -1
        }
    }
}
