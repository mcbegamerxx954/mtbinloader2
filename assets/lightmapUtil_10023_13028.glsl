
vec2 lightmapUtil_10023_13028_ff75e2(vec2 tc1){
    uint pack16 = uint(round(tc1.y * 65535.0));
    uvec2 uv = uvec2(pack16 >> 4u, pack16) & 0x0Fu;
    return clamp(vec2(uv) * 0.066666, 0.0, 1.0);
}
#ifdef a_texcoord1
  #undef a_texcoord1
#endif
#define a_texcoord1 lightmapUtil_10023_13028_ff75e2(a_texcoord1)
    
