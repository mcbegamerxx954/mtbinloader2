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

void cxx_string$init(std::string *s, const std::uint8_t *ptr,
                                std::size_t len) noexcept {
  new (s) std::string(reinterpret_cast<const char *>(ptr), len);
}

void cxx_string$reserve_total(std::string &s,
                                         size_t new_cap) noexcept {
  s.reserve(new_cap);
}

void cxx_string$clear(std::string &s) noexcept {
  s.clear();
}
size_t cxx_string$length(const std::string &s) noexcept {
  return s.size();
}
const char* cxx_string$data(const std::string &s) noexcept {
  return s.data();
}
void cxx_string$destroy(std::string *s) noexcept {
  using std::string;
  s->~string();
}
extern "C" {
void cxx_string$push(std::string &s, const std::uint8_t *ptr,
                                std::size_t len) noexcept {
  s.append(reinterpret_cast<const char *>(ptr), len);
}
}
}
