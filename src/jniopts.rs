use std::sync::{LazyLock, Mutex};

use jni::{
    objects::{AsJArrayRaw, JObject, JObjectArray, JPrimitiveArray, JString},
    sys::{jboolean, JNI_TRUE},
    JNIEnv,
};
use materialbin::{MinecraftVersion, ALL_VERSIONS};
pub struct Options {
    pub handle_lightmaps: bool,
    pub handle_texturelods: bool,
    pub autofixer_versions: Vec<MinecraftVersion>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            handle_lightmaps: true,
            handle_texturelods: true,
            autofixer_versions: ALL_VERSIONS.to_vec(),
        }
    }
}
pub static OPTS: LazyLock<Mutex<Options>> = LazyLock::new(|| Mutex::new(Options::default()));
#[no_mangle]
extern "C" fn Java_dev_faizul726_mbloaderjetpack_launcherUtils_libBindings_SetAutofixVersions(
    mut env: JNIEnv,
    _thiz: JObject,
    versions: jni::objects::JObjectArray,
) {
    let sus = env.get_array_length(&versions).unwrap();
    let mut rs_versions = Vec::new();
    for index in 0..sus {
        let string = env.get_object_array_element(&versions, index).unwrap();
        let string: JString = string.into();
        //        if !env.is_instance_of(string, "String")
        let sus = env.get_string(&string).unwrap();
        rs_versions.push(version_from_string(sus.to_str().unwrap()).unwrap());
    }
    OPTS.lock().unwrap().autofixer_versions = rs_versions;
}
fn version_from_string(string: &str) -> Option<MinecraftVersion> {
    let mcversion = match string {
        "v1.18.30" => MinecraftVersion::V1_18_30,
        "v1.19.60" => MinecraftVersion::V1_19_60,
        "v1.20.80" => MinecraftVersion::V1_20_80,
        "v1.21.20" => MinecraftVersion::V1_21_20,
        "v1.21.110+" => MinecraftVersion::V1_21_110,
        _ => return None,
    };
    Some(mcversion)
}
#[no_mangle]
extern "C" fn Java_dev_faizul726_mbloaderjetpack_launcherUtils_libBindings_SetLightmapAutofixer(
    mut _env: JNIEnv,
    _thiz: JObject,
    on: jboolean,
) {
    OPTS.lock().unwrap().handle_lightmaps = on == JNI_TRUE;
}
#[no_mangle]
extern "C" fn Java_dev_faizul726_mbloaderjetpack_launcherUtils_libBindings_SetTextureLodAutofixer(
    mut _env: JNIEnv,
    _thiz: JObject,
    on: jboolean,
) {
    OPTS.lock().unwrap().handle_texturelods = on == JNI_TRUE;
}
