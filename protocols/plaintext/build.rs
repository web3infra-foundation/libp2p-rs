fn main() {
    prost_build::compile_protos(&["src/handshake/structs.proto"], &["src/handshake"]).unwrap();
}
