# MaterialBinLoader 2
A well optimized and featureful loader for the block game

# Features
- Supports loading materialbins,camerafiles, and replacing existing oreui files
- Low file loading overhead
- Can automatically autofix shaders as possible most of the time (with caveats)
- Highly modifiable sourcecode
- Low size (300-450kb)

> [!WARNING]
> Autofix cannot bring your HAL (shaders folder) shaders back

> [!CAUTION]
> MBL2 is not responsible for what happens to your shaders after it breaks because of a minecraft update
> autofix may break or imperfectly update some shaders
> if your shader breaks, please update it from the shader developer's page

# Supported platforms
- Android arm64
- Android arm32
- Chromeos/android x86_64 (untested)

# Building
## Requirements
- Rust (latest as possible)
- Your target android architecture's rust target installed
- Ndk r25+ installed

## Building the .so
``` bash
cargo build --release --target {android target triple here}
````
