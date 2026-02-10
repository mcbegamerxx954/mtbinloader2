
vec2 lightmapUtil_11020_13028_994b3a(vec2 tc1){
    uint pack16 = uint(round(tc1.y * 65535.0));
    uvec2 uv = uvec2(pack16 >> 4u, pack16) & 0x0Fu;
    return vec2(float((uv.y << 4u) | uv.x) / 255.0, 0.0);
}
#ifdef a_texcoord1
  #undef a_texcoord1
#endif
#define a_texcoord1 lightmapUtil_11020_13028_994b3a(a_texcoord1)
    
