
#[cfg(not(feature = "library"))]
fn main() -> std::io::Result<()> {
    ew::runtime::update_data_path(&ew::get_args().path);
    ew::run_server(false)
}

#[cfg(feature = "library")]
fn main() {
    panic!("Compiled with the library feature! You should load the shared object library and call the exported methods there.");
}
