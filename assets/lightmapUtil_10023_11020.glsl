
vec2 lightmapUtil_10023_11020_2129c0(vec2 tc1){
    uint pack8 = uint(floor(tc1.x * 255.0));
    uvec2 uv = uvec2(pack8, pack8 >> 4u) & 0x0Fu;
    return clamp(vec2(uv) * 0.0625, 0.0, 1.0);
}
#ifdef a_texcoord1
  #undef a_texcoord1
#endif
#define a_texcoord1 lightmapUtil_10023_11020_2129c0(a_texcoord1)
    
