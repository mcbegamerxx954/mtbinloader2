use crate::cpp_string::{ResourceLocation, StackString};
use cxx::CxxString;
use ndk::asset::AssetManager;
use std::{
    io::{self, Cursor, Read, Seek, Write},
    ops::{Deref, DerefMut},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    pin::Pin,
    sync::atomic::Ordering,
};

pub enum BufferCursor {
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
    pub fn position(&self) -> u64 {
        match self {
            Self::Vec(v) => v.position(),
            Self::Cxx(cxx) => cxx.position(),
        }
    }
    pub fn get_ref(&self) -> &[u8] {
        match self {
            Self::Vec(v) => v.get_ref(),
            Self::Cxx(cxx) => cxx.get_ref().as_ref(),
        }
    }
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

pub struct FileLoader {
    pub last_buffer: Option<Buffer>,
}
impl FileLoader {
    pub fn new() -> Self {
        Self { last_buffer: None }
    }
    pub fn get_file(&mut self, path: &Path, manager: AssetManager) -> Option<Buffer> {
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
pub struct Buffer {
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
