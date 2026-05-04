use jni::{Env, EnvUnowned};
use jni::objects::{JClass, JString};
use jni::sys::jstring;
use jni::errors::ThrowRuntimeExAndDefault;
use std::thread;
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
pub extern "system" fn Java_one_ethanthesleepy_androidew_BackgroundService_startServer<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
    data_path: JString<'local>,
    easter: bool,
) -> jstring {
    crate::runtime::set_easter_mode(easter);

    unowned_env
        .with_env(|env: &mut Env<'local>| -> jni::errors::Result<jstring> {
            let data_path: String = data_path.to_string();
            crate::runtime::update_data_path(&data_path);

            let output = JString::from_str(env, "Azunyannnn~")?;

            thread::spawn(|| {
                run_server(true).unwrap();
            });
            log_to_logcat!("ew", "running");

            Ok(output.into_raw())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_one_ethanthesleepy_androidew_BackgroundService_stopServer<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> jstring {
    stop_server();

    unowned_env
        .with_env(|env: &mut Env<'local>| -> jni::errors::Result<jstring> {
            let output = JString::from_str(env, "I like Yui!")?;
            Ok(output.into_raw())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_one_ethanthesleepy_androidew_BackgroundService_setEasterMode<'local>(
    _unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
    easter: bool,
) {
    crate::runtime::set_easter_mode(easter);
}
