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
static IS_1_21_100: AtomicBool = AtomicBool::new(false);
static IS_1_21_130: AtomicBool = AtomicBool::new(false);

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
                if memchr::memmem::find(&buf, b"65535.0").is_some() {
                    log::info!("Mc version is 1_21_130 or higher");
                    IS_1_21_130.store(true, Ordering::Release);
                } // else {
                    log::info!("Mc version is 1_21_100 or higher");
                    IS_1_21_100.store(true, Ordering::Release);
                // }
            }
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
                log::trace!("[{version}] Parsing failed: {e}");
                continue;
            }
        };
        log::info!("Processing mtbin: {} [{version}]", material.name);
        // we detect jaundice using finder instead of mtbin version
        let needs_lightmap_fix = material.name == "RenderChunk"
            && IS_1_21_100.load(Ordering::Acquire)
            //&& version <= MinecraftVersion::V1_21_110
            && (version < MinecraftVersion::V26_0_24 //isolation
                || mcver < MinecraftVersion::V26_0_24) //toleration
            && opts.handle_lightmaps;
        let needs_sampler_fix = material.name == "RenderChunk"
            && mcver >= MinecraftVersion::V1_20_80
            && version <= MinecraftVersion::V1_19_60
            && opts.handle_texturelods;
        if version == mcver && !needs_lightmap_fix && !needs_sampler_fix {
            log::info!("Did not fix mtbin, mtversion: {version}");
            return None; // Prevent some work
        }
        if needs_lightmap_fix {
            log::warn!("Had to fix lightmaps for RenderChunk");
            let mut changed = 0;
            handle_lightmaps(&mut material, version, &mut changed);
            log::info!("autofix have changed {changed} passes");
            if changed == 0 && version == mcver {
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
    log::error!("It seems like we couldnt process the materialbin");
    None
}
fn handle_lightmaps(
    materialbin: &mut CompiledMaterialDefinition,
    version: MinecraftVersion,
    changed: &mut i32,
) {
    let main_start = Finder::new(b"void main");
    let lightmap_10023_11020: &[u8] = include_bytes!("../assets/lightmapUtil_10023_11020.glsl");
    let lightmap_10023_13028: &[u8] = include_bytes!("../assets/lightmapUtil_10023_13028.glsl");
    let lightmap_11020_13028: &[u8] = include_bytes!("../assets/lightmapUtil_11020_13028.glsl");
    let lightmap_13028_11020: &[u8] = include_bytes!("../assets/lightmapUtil_13028_11020.glsl");
    let mc_1_21_130 = IS_1_21_130.load(Ordering::Acquire);
    let legacy_assign = Finder::new(b"v_lightmapUV = a_texcoord1;");
    let legacy_assign2 = Finder::new(b"v_lightmapUV=a_texcoord1;");
    let vanilla_fix = Finder::new(b"65535.0");
    let newbx_fix = Finder::new("vec2(256.0, 4096.0)");
    for (_, scode) in materialbin
        .passes
        .iter_mut()
        .filter(|(passes, _)| *passes != "DepthOnly" && *passes != "DepthOnlyOpaque")
        .flat_map(|(_, pass)| &mut pass.variants)
        .flat_map(|variants| &mut variants.shader_codes)
        .filter(|(stage, _)| { 
            stage.stage == ShaderStage::Vertex
            && (stage.platform == ShaderCodePlatform::Essl100 
                || stage.platform == ShaderCodePlatform::Essl300)
            && (stage.platform_name == "ESSL_100" 
                || stage.platform_name == "ESSL_300"
                || stage.platform_name == "ESSL_310")
        })
    {
        let code = &scode.bgfx_shader_data;
        let shader_1_21_100 = legacy_assign.find(code).is_none() && legacy_assign2.find(code).is_none();
        let shader_1_21_130 = vanilla_fix.find(code).is_some() || newbx_fix.find(code).is_some();
        let replace_with: &[u8];
        //mcver is already 1.21.100 or higher
        if shader_1_21_100 {    
            if shader_1_21_130 { //shader == 1.21.130
                if mc_1_21_130 {
                    log::info!("Skipping replacement!! 1_21_130");
                    continue;
                } else {
                    log::warn!("autofix: 13028 -> 11020");
                    replace_with = lightmap_13028_11020;
                }
            } else { //shader == 1.21.110
                if mc_1_21_130 {
                    log::warn!("autofix: 11020 -> 13028");
                    replace_with = lightmap_11020_13028;
                } else {
                    log::info!("Skipping replacement!! 1_21_110");
                    continue;
                }
            }
        } else { //shader <= 1.21.90
            if mc_1_21_130 {
                log::warn!("autofix: 10023 -> 13028");
                replace_with = lightmap_10023_13028;
            } else {
                log::warn!("autofix: 10023 -> 11020");
                replace_with = lightmap_10023_11020;
            }
        }
        *changed += 1;
        add_bytes_before(&mut scode.bgfx_shader_data, &main_start, replace_with);
    }
}
// fn cmp_ign_whitespace(str1: &str, str2: &str) -> bool {
//     str1.chars().filter(|c| !c.is_whitespace()).eq(str2.chars())
// }
fn handle_samplers(materialbin: &mut CompiledMaterialDefinition) {
    let main_start = Finder::new(b"void main ()");
    let replace_with: &[u8] = b"
#if __VERSION__ >= 300
  #define texture(tex,uv) vec4(texture(tex,uv).rgb,textureLod(tex,uv,0.0).a)
#else
  #define texture2D(tex,uv) vec4(texture2D(tex,uv).rgb,texture2DLod(tex,uv,0.0).a)
#endif
";
    for (_, scode) in materialbin
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
        add_bytes_before(&mut scode.bgfx_shader_data, &main_start, replace_with);
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
