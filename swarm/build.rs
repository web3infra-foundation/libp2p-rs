fn main() {
    prost_build::compile_protos(&["src/identify/structs.proto"], &["src"]).unwrap();
}
