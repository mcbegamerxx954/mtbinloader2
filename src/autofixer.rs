use crate::{jniopts::OPTS, LockResultExt};
use materialbin::{
    bgfx_shader::BgfxShader,
    pass::{ShaderCodePlatform, ShaderStage},
    CompiledMaterialDefinition, MinecraftVersion,
};
use memchr::memmem::Finder;
use ndk::asset::{Asset, AssetManager};
use scroll::Pread;
use std::io::Read;
use std::{
    //    cmp::Ordering,
    ffi::CStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
};

// The Minecraft version we will use to port shaders to
static MC_VERSION: OnceLock<Option<MinecraftVersion>> = OnceLock::new();
static MC_IS_1_21_100: AtomicBool = AtomicBool::new(false);
static MC_IS_1_21_130: AtomicBool = AtomicBool::new(false);

fn get_current_mcver(man: ndk::asset::AssetManager) -> Option<MinecraftVersion> {
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
                MC_IS_1_21_100.store(true, Ordering::Release);
                log::info!("Mc version is 1_21_100 or higher");
            } // else { panic!("mc version unsupported! cannot find:v_dithering"); };
            if memchr::memmem::find(&buf, b"a_texcoord1 * 65535.0").is_some() {
                MC_IS_1_21_130.store(true, Ordering::Release);
                log::info!("Mc version is 1_21_130 or higher");
            } // else { panic!("mc version unsupported! cannot find:a_texcoord1*65535.0"); };
            return Some(version);
        };
    }
    log::warn!("Cannot detect mc version, autofix disabled");
    None
}

// Try to open UIText.material.bin to guess Minecraft shader version
fn get_uitext(man: ndk::asset::AssetManager) -> Option<Asset> {
    const NEW: &CStr = c"assets/renderer/materials/RenderChunk.material.bin";
    const OLD: &CStr = c"renderer/materials/RenderChunk.material.bin";
    for path in [NEW, OLD] {
        if let Some(asset) = man.open(path) {
            return Some(asset);
        }
    }
    None
}
pub fn process_material(man: AssetManager, data: &[u8]) -> Option<Vec<u8>> {
    let mcver = MC_VERSION.get_or_init(|| get_current_mcver(man));
    // Just ignore if no Minecraft version was found
    let mcver = (*mcver)?;
    let opts = OPTS.lock().ignore_poison();
    for version in opts.autofixer_versions.iter() {
        let version = *version;
        let mut material: CompiledMaterialDefinition = match data.pread_with(0, version) {
            Ok(data) => data,
            Err(e) => {
                log::trace!("[version] Parsing failed: {e}");
                continue;
            }
        };
        let needs_lightmap_fix = MC_IS_1_21_100.load(Ordering::Acquire)
            // && version != MinecraftVersion::V1_21_110
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
        let mut changed = 0;
        if needs_lightmap_fix {
            log::warn!("Had to fix lightmaps for RenderChunk");
            handle_lightmaps(&mut material, version, &mut changed);
            log::warn!("autofix have changed {changed} passes");
            if changed == 0 {
                log::info!("nothing changed, skip writting");
                return None; //shader is already 1.21.100+
            }
        }
        if needs_sampler_fix {
            log::warn!("Had to fix mipmap levels for RenderChunk");
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
fn handle_lightmaps(
    materialbin: &mut CompiledMaterialDefinition,
    version: MinecraftVersion,
    changed: &mut i32,
) {
    //log::info!("mtbinloader25 handle_lightmaps");
    let pattern = b"void main";
    let lightmap_10023_11020: &[u8] = include_bytes!("../assets/lightmapUtil_10023_11020.glsl");
    let lightmap_10023_13028: &[u8] = include_bytes!("../assets/lightmapUtil_10023_13028.glsl");
    let lightmap_11020_13028: &[u8] = include_bytes!("../assets/lightmapUtil_11020_13028.glsl");
    let main_start = Finder::new(pattern);
    let legacy_assign = Finder::new(b"v_lightmapUV = a_texcoord1;");
    let legacy_assign2 = Finder::new(b"v_lightmapUV=a_texcoord1;");
    let magic_fix_number = Finder::new(b"65535.0");
    let newbx_fix = Finder::new("vec2(256.0, 4096.0)");
    for (_, scode) in materialbin
        .passes
        .iter_mut()
        .flat_map(|(_, pass)| &mut pass.variants)
        .flat_map(|variants| &mut variants.shader_codes)
        .filter(|(stage, _)| {
            stage.stage == ShaderStage::Vertex
                && (stage.platform == ShaderCodePlatform::Essl100
                    || stage.platform == ShaderCodePlatform::Essl300)
        })
    {
        // if version == MinecraftVersion::V1_21_20
        // if stage.platform == ShaderCodePlatform::Essl100
        // if stage.platform_name != "ESSL_310" && (
        // if version != MinecraftVersion::V1_21_110
        // log::warn!("Skipping replacement due to not existing lightmap UV assignment");
        // let mut bgfx: BgfxShader = code.bgfx_shader_data.pread(0).unwrap();
        //        let blob = &mut code.bgfx_shader_data;
        // let Ok(mut bgfx) = blob.pread::<BgfxShader>(0) else {
        //     continue;
        // };
        let code = &scode.bgfx_shader_data;
        let is_1_21_130 = MC_IS_1_21_130.load(Ordering::Acquire);
        let has_fix = magic_fix_number.find(code).is_some() || newbx_fix.find(code).is_some();
        let replace_with: &[u8];
        // shader is 1-21-100 or above
        if legacy_assign.find(code).is_none() && legacy_assign2.find(code).is_none() {
            if version >= MinecraftVersion::V1_21_110 && has_fix {
                //shader is already 1-21-130
                log::info!("finder already 1_21_130!!! Skipping replacement...");
                continue;
            } else if is_1_21_130 {
                log::info!("autofix: 11020 -> 13028");
                replace_with = lightmap_11020_13028;
            } else {
                log::info!("finder already 1_21_110!!! Skipping replacement...");
                continue;
            }
        } else if is_1_21_130 {
            log::info!("autofix: 10023 -> 13028");
            replace_with = lightmap_10023_13028;
        } else {
            log::info!("autofix: 10023 -> 11020");
            replace_with = lightmap_10023_11020;
        }
        *changed += 1;
        //log::info!("autofix is doing lightmap replacing...");
        add_bytes_before(&mut scode.bgfx_shader_data, &main_start, replace_with);
        //        blob.clear();
        //        let _unused = bgfx.write(blob);
    }
}
// fn cmp_ign_whitespace(str1: &str, str2: &str) -> bool {
//     str1.chars().filter(|c| !c.is_whitespace()).eq(str2.chars())
// }
fn handle_samplers(materialbin: &mut CompiledMaterialDefinition) {
    //log::info!("mtbinloader25 handle_samplers");
    let pattern = b"void main ()";
    let replace_with: &[u8] = b"
#if __VERSION__ >= 300
 #define texture(tex,uv) textureLod(tex,uv,0.0)
#else
 #define texture2D(tex,uv) texture2DLod(tex,uv,0.0)
#endif";
    let finder = Finder::new(pattern);
    for (_, code) in materialbin
        .passes
        .iter_mut()
        .filter(|(passes, _)| *passes == "AlphaTest" || *passes == "Opaque")
        .flat_map(|(_, pass)| &mut pass.variants)
        .flat_map(|variants| &mut variants.shader_codes)
        .filter(|(stage, _)| {
            stage.stage == ShaderStage::Fragment && stage.platform_name == "ESSL_100"
        })
    {
        log::info!("handling texture sampler to disable mipmap...");
        let mut bgfx: BgfxShader = code
            .bgfx_shader_data
            .pread(0)
            .expect("Failed reading bgfx shader data");
        add_bytes_before(&mut bgfx.code, &finder, replace_with);
        code.bgfx_shader_data.clear();
        bgfx.write(&mut code.bgfx_shader_data)
            .expect("bgfx shader Write error... huh");
    }
}

fn add_bytes_before(codebuf: &mut Vec<u8>, finder: &Finder, replace_with: &[u8]) {
    let position = match finder.find(codebuf) {
        Some(yay) => yay,
        None => return,
    };
    let previous = position;
    codebuf.splice(previous..previous, replace_with.iter().cloned());
}
