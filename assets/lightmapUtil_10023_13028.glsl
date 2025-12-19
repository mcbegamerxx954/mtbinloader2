
vec2 lightmapUtil_10023_13028_190d99(vec2 tc1){
    uint tmp = uint(round(tc1.y * 65535.0));
    return clamp(vec2(uvec2(
        tmp >> 4u,
        tmp & 15u
    ) & 15u) * 0.066666, 0.0, 1.0);
}
#ifdef a_texcoord1
 #undef a_texcoord1
#endif
#define a_texcoord1 lightmapUtil_10023_13028_190d99(a_texcoord1)


