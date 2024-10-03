#include <cstddef>
#include <cstdint>
#include <string>
// This is needed vecause rust cannot own c++ strings
// Which is required for our usages
struct ResourceLocation {
    int32_t mFileSystem = 0;
    std::string mPath;
    uint64_t mPathHash = 0;
    uint64_t mFullHash = 0;

    ResourceLocation() {}
    ResourceLocation(const std::string& path) : mPath(path) {}
};
extern "C" {
  ResourceLocation* resource_location_init(const char* strptr, size_t size) {
    ResourceLocation* loc = new ResourceLocation;
    std::string rust_str(strptr, size);
    loc->mPath.assign(rust_str);
    return loc;
  }
  void resource_location_free(ResourceLocation* loc) {
    delete loc;
  }
}
