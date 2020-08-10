use std::env;

#[allow(clippy::inconsistent_digit_grouping)]
fn main() {
    if let Ok(v) = env::var("DEP_OPENSSL_VERSION_NUMBER") {
        let version = u64::from_str_radix(&v, 16).unwrap();

        if version >= 0x1_01_00_00_0 {
            println!("cargo:rustc-cfg=ossl110");
        }
    }
    prost_build::compile_protos(&["src/crypto/keys.proto"], &["src/crypto"]).unwrap();
    prost_build::compile_protos(&["src/handshake/structs.proto"], &["src/handshake"]).unwrap();
}
