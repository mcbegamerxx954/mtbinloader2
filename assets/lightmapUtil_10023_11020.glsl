
vec2 lightmapUtil_10023_11020_ead63a(vec2 tc1){
    uint tmp = uint(floor(tc1.x * 255.0));
    return clamp(vec2(uvec2(
        tmp & 15u,
        tmp >> 4u
    ) & 15u) * 0.0625, 0.0, 1.0);
}
#ifdef a_texcoord1
 #undef a_texcoord1
#endif
#define a_texcoord1 lightmapUtil_10023_11020_ead63a(a_texcoord1)
    

