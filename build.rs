fn main() {
	// sherpa-onnx / onnxruntime shared libs live next to the binary.
	println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
}
