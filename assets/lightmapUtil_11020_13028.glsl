
vec2 lightmapUtil_11020_13028_274db2(vec2 tc1){
    uvec2 uv = uvec2(
        uint(round(tc1.y * 65535.0)) >> 4u,
        uint(round(tc1.y * 65535.0)) & 15u
    ) & 15u;
    return vec2(float((uv.y << 4u) | uv.x) / 255.0, 0.0);
}
#ifdef a_texcoord1
 #undef a_texcoord1
#endif
#define a_texcoord1 lightmapUtil_11020_13028_274db2(a_texcoord1)

