//Explanation: Aasset is NOT thread-safe anyways so we will not try adding thread safety either
#![allow(static_mut_refs)]

use crate::{
    cpp_string::{ResourceLocation, StackString},
    jniopts::OPTS,
};
use asset_overlay::{FileProvider, SyncFile};
use cxx::CxxString;
use libc::{c_char, c_int, c_void, off64_t, off_t, size_t};
use materialbin::{
    bgfx_shader::BgfxShader, pass::ShaderStage, CompiledMaterialDefinition, MinecraftVersion,
};
use memchr::memmem::Finder;
use ndk::asset::{Asset, AssetManager};
use ndk_sys::{AAsset, AAssetManager};
use once_cell::sync::Lazy;
use scroll::Pread;
use std::{
    cell::UnsafeCell,
    collections::HashMap,
    ffi::{CStr, OsStr},
    io::{self, Cursor, Read, Seek, Write},
    ops::{Deref, DerefMut},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        LazyLock, Mutex, OnceLock,
    },
};
// The Minecraft version we will use to port shaders to
static MC_VERSION: OnceLock<Option<MinecraftVersion>> = OnceLock::new();
static IS_1_21_100: AtomicBool = AtomicBool::new(false);
fn get_current_mcver(man: &AssetManager) -> Option<MinecraftVersion> {
    let mut file = match get_uitext(man) {
        Some(asset) => asset,
        None => {
            log::error!("Shader fixing is disabled as RenderChunk was not found");
            return None;
        }
    };
    let mut buf = Vec::with_capacity(file.length());
    if let Err(e) = file.read_to_end(&mut buf) {
        log::error!("Something is wrong with AssetManager, mc detection failed: {e}");
        return None;
    };

    for version in materialbin::ALL_VERSIONS.into_iter().rev() {
        if let Ok(_shader) = buf.pread_with::<CompiledMaterialDefinition>(0, version) {
            log::info!("Mc version is {version}");
            if memchr::memmem::find(&buf, b"v_dithering").is_some() {
                log::warn!("mc is 1.21.100 and higher");
                IS_1_21_100.store(true, Ordering::Release);
            }
            return Some(version);
        };
    }
    log::warn!("Cannot detect mc version, autofix disabled");
    None
}

// Try to open UIText.material.bin to guess Minecraft shader version
fn get_uitext(man: &AssetManager) -> Option<Asset> {
    const NEW: &CStr = c"assets/renderer/materials/RenderChunk.material.bin";
    const OLD: &CStr = c"renderer/materials/RenderChunk.material.bin";
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
fn process_material(man: &AssetManager, data: &[u8]) -> Option<Vec<u8>> {
    let mcver = MC_VERSION.get_or_init(|| get_current_mcver(man));
    // Just ignore if no Minecraft version was found
    let mcver = (*mcver)?;
    let opts = OPTS.lock().unwrap();
    for version in opts.autofixer_versions.iter() {
        let version = *version;
        let mut material: CompiledMaterialDefinition = match data.pread_with(0, version) {
            Ok(data) => data,
            Err(e) => {
                log::trace!("[version] Parsing failed: {e}");
                continue;
            }
        };
        let needs_lightmap_fix = IS_1_21_100.load(Ordering::Acquire)
            && version != MinecraftVersion::V1_21_110
            && (material.name == "RenderChunk" || material.name == "RenderChunkPrepass")
            && opts.handle_lightmaps;
        let needs_sampler_fix = material.name == "RenderChunk"
            && mcver >= MinecraftVersion::V1_20_80
            && version <= MinecraftVersion::V1_19_60
            && opts.handle_texturelods;
        // Prevent some work
        if version == mcver && !needs_lightmap_fix && !needs_sampler_fix {
            log::info!("Did not fix mtbin, mtversion: {version}");
            return None;
        }
        if needs_lightmap_fix {
            handle_lightmaps(&mut material);
            log::warn!("Had to fix lightmaps for RenderChunk");
        }
        if needs_sampler_fix {
            handle_samplers(&mut material);
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
fn handle_lightmaps(materialbin: &mut CompiledMaterialDefinition) {
    let finder = Finder::new(b"void main");
    // very bad code please help
    let finder1 = Finder::new(b"v_lightmapUV = a_texcoord1;");
    let finder2 = Finder::new(b"v_lightmapUV=a_texcoord1;");
    let finder3 = Finder::new(b"#define a_texcoord1 ");
    let replace_with = b"
#define a_texcoord1 vec2(fract(a_texcoord1.x*15.9375)+0.0001,floor(a_texcoord1.x*15.9375)*0.0625+0.0001)
void main";
    for (_, pass) in &mut materialbin.passes {
        for variants in &mut pass.variants {
            for (stage, code) in &mut variants.shader_codes {
                if stage.stage == ShaderStage::Vertex {
                    let blob = &mut code.bgfx_shader_data;
                    let Ok(mut bgfx) = blob.pread::<BgfxShader>(0) else {
                        continue;
                    };
                    if finder3.find(&bgfx.code).is_some()
                        || (finder1.find(&bgfx.code).is_none()
                            && finder2.find(&bgfx.code).is_none())
                    {
                        continue;
                    };
                    replace_bytes(&mut bgfx.code, &finder, b"void main", replace_with);

                    blob.clear();
                    let _unused = bgfx.write(blob);
                }
            }
        }
    }
}
// fn cmp_ign_whitespace(str1: &str, str2: &str) -> bool {
//     str1.chars().filter(|c| !c.is_whitespace()).eq(str2.chars())
// }
fn handle_samplers(materialbin: &mut CompiledMaterialDefinition) {
    let pattern = b"void main ()";
    let replace_with = b"
#if __VERSION__ >= 300
 #define texture(tex,uv) textureLod(tex,uv,0.0)
#else
 #define texture2D(tex,uv) texture2DLod(tex,uv,0.0)
#endif
void main ()";
    let finder = Finder::new(pattern);
    for (_passes, pass) in &mut materialbin.passes {
        if _passes == "AlphaTest" || _passes == "Opaque" {
            for variants in &mut pass.variants {
                for (stage, code) in &mut variants.shader_codes {
                    if stage.stage == ShaderStage::Fragment && stage.platform_name == "ESSL_100" {
                        log::info!("handle_samplers");
                        let mut bgfx: BgfxShader = code.bgfx_shader_data.pread(0).unwrap();
                        replace_bytes(&mut bgfx.code, &finder, pattern, replace_with);
                        code.bgfx_shader_data.clear();
                        bgfx.write(&mut code.bgfx_shader_data).unwrap();
                    }
                }
            }
        }
    }
}

fn replace_bytes(codebuf: &mut Vec<u8>, finder: &Finder, pattern: &[u8], replace_with: &[u8]) {
    let sus = match finder.find(codebuf) {
        Some(yay) => yay,
        None => {
            println!("oops");
            return;
        }
    };
    codebuf.splice(sus..sus + pattern.len(), replace_with.iter().cloned());
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
pub struct Mbl2Backend {}
impl FileProvider for Mbl2Backend {
    fn get_file(&mut self, path: &Path, manager: &AssetManager) -> Option<Box<SyncFile>> {
        let stripped = path.strip_prefix("assets/").unwrap_or(path);
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
                    match process_material(&manager, cxx_ptr.as_bytes()) {
                        Some(updated) => BufferCursor::Vec(Cursor::new(updated)),
                        None => BufferCursor::Cxx(Cursor::new(cxx_storage)),
                    }
                } else {
                    BufferCursor::Cxx(Cursor::new(cxx_storage))
                };
                //                let cache = Buffer::new(path.to_path_buf(), buffer);
                // ResourceLocation gets dropped (also cxx_storage if its not needed)
                return Some(Box::new(buffer));
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
