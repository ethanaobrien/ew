use crate::lock_onto_mutex;
use lazy_static::lazy_static;
use std::sync::Mutex;

lazy_static! {
    static ref RUNNING: Mutex<bool> = Mutex::new(false);
}

pub fn set_running(running: bool) {
    let mut result = lock_onto_mutex!(RUNNING);
    *result = running;
}

pub fn get_running() -> bool {
    let result = lock_onto_mutex!(RUNNING);
    *result
}
