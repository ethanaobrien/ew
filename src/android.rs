use jni::JNIEnv;
use jni::objects::{JClass};
use jni::sys::{jstring, jboolean};
use std::thread;
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

#[unsafe(no_mangle)]
extern "C" fn Java_one_ethanthesleepy_androidew_BackgroundService_startServer<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    data_path: JString<'local>,
    easter: jboolean
) -> jstring {
    crate::runtime::set_easter_mode(easter != 0);

    let data_path: String = env.get_string(&data_path).unwrap().into();
    crate::runtime::update_data_path(&data_path);

    let output = env.new_string(String::from("Azunyannnn~")).unwrap();
    thread::spawn(|| {
        run_server(true).unwrap();
    });
    log_to_logcat!("ew", "running");
    output.into_raw()
}

#[unsafe(no_mangle)]
extern "C" fn Java_one_ethanthesleepy_androidew_BackgroundService_stopServer<'local>(env: JNIEnv<'local>, _class: JClass<'local>) -> jstring {
    stop_server();
    let output = env.new_string(String::from("I like Yui!")).unwrap();
    output.into_raw()
}


#[unsafe(no_mangle)]
extern "C" fn Java_one_ethanthesleepy_androidew_BackgroundService_setEasterMode<'local>(_env: JNIEnv<'local>, _class: JClass<'local>, easter: jboolean) {
    crate::runtime::set_easter_mode(easter != 0);
}
