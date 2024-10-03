fn main() {
    cc::Build::new()
        .cpp(true)
        .file("src/string.cpp")
        .compile("stringstub");
}
