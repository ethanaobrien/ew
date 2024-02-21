use openssl::symm::{Cipher, Crypter, Mode};
use openssl::error::ErrorStack;
use base64::{Engine as _, engine::general_purpose};

const IV_LENGTH: usize = 16;
const KEY: &str = "3559b435f24b297a79c68b9709ef2125";

pub fn decrypt_packet(base64_input: &str) -> Result<String, ErrorStack> {
    let base64_buffer = general_purpose::STANDARD.decode(base64_input).unwrap();

    let decryption_iv = &base64_buffer[..IV_LENGTH];
    let ciphertext = &base64_buffer[IV_LENGTH..];

    let cipher = Cipher::aes_256_cbc();
    let mut decrypter = Crypter::new(cipher, Mode::Decrypt, KEY.as_bytes(), Some(decryption_iv))?;

    let mut decrypted_data = vec![0u8; ciphertext.len() + cipher.block_size()];
    let mut decrypted_len = decrypter.update(ciphertext, &mut decrypted_data)?;
    decrypted_len += decrypter.finalize(&mut decrypted_data[decrypted_len..])?;
    
    decrypted_data.truncate(decrypted_len);

    Ok(String::from_utf8(decrypted_data).unwrap())
}
