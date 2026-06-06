
#[cfg(not(feature = "library"))]
fn main() -> std::io::Result<()> {
    let args = ew::get_args();
    ew::runtime::update_data_path(&args.path);
    ew::runtime::update_masterdata_path(&args.masterdata);
    ew::runtime::update_mod_paths(&args.mods);
    ew::run_server(false)
}

#[cfg(feature = "library")]
fn main() {
    panic!("Compiled with the library feature! You should load the shared object library and call the exported methods there.");
}
