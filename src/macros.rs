// include_file macro: includes a file compressed at compile time, and decompresses it on reference. Decreases binary size
#[macro_export]
macro_rules! include_file {
    ( $s:expr ) => {
        {
            let file = include_flate_codegen::deflate_file!($s);
            let ret = $crate::macros::decode(file);
            std::string::String::from_utf8(ret).unwrap()
        }
    };
}

pub fn decode(bytes: &[u8]) -> Vec<u8> {
    use std::io::{Cursor, Read};

    let mut dec = libflate::deflate::Decoder::new(Cursor::new(bytes));
    let mut ret = Vec::new();
    dec.read_to_end(&mut ret).unwrap();
    ret
}

#[macro_export]
macro_rules! lock_onto_mutex {
    ($mutex:expr) => {{
        loop {
            match $mutex.lock() {
                Ok(value) => {
                    break value;
                }
                Err(_) => {
                    $mutex.clear_poison();
                    std::thread::sleep(std::time::Duration::from_millis(15));
                }
            }
        }
    }};
}
