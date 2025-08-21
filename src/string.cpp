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
  ResourceLocation* resource_location_init() {
    ResourceLocation* loc = new ResourceLocation;
    loc->mPath = "";
    return loc;
  }
  std::string* resource_location_path(ResourceLocation* loc) {
    return &loc->mPath;
  }
  void resource_location_free(ResourceLocation* loc) {
    delete loc;
  }
}
