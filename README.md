# MaterialBinLoader 2
A well optimized and featureful loader for the block game

# Features
- Supports loading materialbins,camerafiles, and replacing existing oreui files
- Low loading overhead
- Can automatically fix shaders as possible most of the time (with caveats)
- Highly modifiable sourcecode
- Low size (300-450kb)

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
