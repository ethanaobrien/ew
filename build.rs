fn main() {
    let target = std::env::var("TARGET").unwrap();
    if target == "aarch64-linux-android" {
        println!("cargo:rerun-if-changed=build.rs");
        println!("cargo:rustc-link-arg=-Wl,-z,max-page-size=65536");

        cc::Build::new()
            .file("src/log.c")
            .compile("libc_code.a");
        println!("cargo:rerun-if-changed=src/log.c");
    }
}
