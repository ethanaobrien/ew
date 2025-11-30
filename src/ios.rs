#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
#[unsafe(link_section = "__DATA,__mod_init_func")]
pub static INITIALIZER: extern "C" fn() = main;

#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn main() {
    let data_path = get_bundle_path().into_os_string().into_string().unwrap();
    crate::runtime::update_data_path(data_path);

    std::thread::spawn(|| {
        crate::run_server(true).unwrap();
    });
}

#[cfg(target_os = "ios")]
use objc2_foundation::{NSFileManager, NSSearchPathDirectory, NSSearchPathDomainMask};

#[cfg(target_os = "ios")]
pub fn get_bundle_path() -> std::path::PathBuf {
    unsafe {
        let manager = NSFileManager::defaultManager();
        let application_support = manager.URLsForDirectory_inDomains(NSSearchPathDirectory::ApplicationSupportDirectory, NSSearchPathDomainMask::UserDomainMask);
        return application_support.to_vec_unchecked()[0].to_file_path().unwrap();
    }
}
