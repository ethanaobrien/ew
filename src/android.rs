use jni::JNIEnv;
use jni::objects::{JClass};
use jni::sys::jstring;
use std::thread;
use std::sync::Once;
use jni::objects::JString;
use crate::{run_server, stop_server};
use std::os::raw::c_char;

#[link(name = "c_code", kind = "static")]
unsafe extern "C" {
    pub fn android_log(tag: *const c_char, message: *const c_char);
}

#[macro_export]
macro_rules! log_to_logcat {
    ($tag:expr, $($arg:tt)*) => {
        let log_message = format!($($arg)*);

        let _ = std::panic::catch_unwind(|| {
            let tag = std::ffi::CString::new($tag).unwrap();
            let message = std::ffi::CString::new(log_message).unwrap();
            unsafe {
                crate::android::android_log(tag.as_ptr(), message.as_ptr());
            }
        });
    };
}

static ANDROID_INIT: Once = Once::new();

// Install a panic hook once so Rust panics surface in logcat instead of being
// silently swallowed at the JNI boundary.
fn android_init() {
    ANDROID_INIT.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            log_to_logcat!("ew", "PANIC: {}", info);
        }));
    });
}

#[unsafe(no_mangle)]
extern "C" fn Java_one_ethanthesleepy_androidew_BackgroundService_startServer<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    config: JString<'local>,
) -> jstring {
    android_init();

    let config: String = env.get_string(&config).unwrap().into();
    crate::runtime::apply_config_json(&config);

    let output = env.new_string(String::from("Azunyannnn~")).unwrap();
    thread::spawn(|| {
        run_server(true).unwrap();
    });
    log_to_logcat!("ew", "running");
    output.into_raw()
}

#[unsafe(no_mangle)]
extern "C" fn Java_one_ethanthesleepy_androidew_BackgroundService_stopServer<'local>(env: JNIEnv<'local>, _class: JClass<'local>) -> jstring {
    android_init();
    stop_server();
    let output = env.new_string(String::from("I like Yui!")).unwrap();
    output.into_raw()
}


#[unsafe(no_mangle)]
extern "C" fn Java_one_ethanthesleepy_androidew_BackgroundService_updateConfig<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    config: JString<'local>,
) {
    android_init();
    let config: String = env.get_string(&config).unwrap().into();
    crate::runtime::apply_config_json(&config);
}
