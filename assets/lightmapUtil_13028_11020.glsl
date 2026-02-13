
vec2 lightmapUtil_13028_11020_db68b5(vec2 tc1){
    uint pack8 = uint(floor(tc1.x * 255.0));
    uvec2 uv = uvec2(pack8, pack8 >> 4u) & 0x0Fu;
    return vec2(0.0, float((uv.x << 4u) | uv.y) / 65535.0);
}
#ifdef a_texcoord1
  #undef a_texcoord1
#endif
#define a_texcoord1 lightmapUtil_13028_11020_db68b5(a_texcoord1)
    
