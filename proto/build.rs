use prost_types::FileDescriptorSet;

const PROTO_FILES: &[&str] = &[
    "vendor/story/evmengine/v1/types/tx.proto",
    "vendor/cosmos/tx/v1beta1/tx.proto",
];

const INCLUDES: &[&str] = &["vendor"];

fn main() {
    let fds = protox_compile();
    prost_build(fds);
}

fn protox_compile() -> FileDescriptorSet {
    protox::compile(PROTO_FILES, INCLUDES).expect("protox failed to build")
}

fn prost_build(fds: FileDescriptorSet) {
    let mut config = prost_build::Config::new();
    config.extern_path(".google.protobuf.Any", "::prost_types::Any");
    config
        .include_file("mod.rs")
        .compile_fds(fds)
        .expect("prost failed");
}
