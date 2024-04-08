use openssl::symm::{Cipher, Crypter, Mode};
use openssl::error::ErrorStack;
use base64::{Engine as _, engine::general_purpose};
use rand::Rng;

const IV_LENGTH: usize = 16;
const KEY: &str = "3559b435f24b297a79c68b9709ef2125";

pub fn decrypt_packet(base64_input: &str) -> Result<String, ErrorStack> {
    if base64_input.len() < IV_LENGTH + 1 {
        return Ok(String::new());
    }
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

pub fn encrypt_packet(input: &str) -> Result<String, ErrorStack> {
    let cipher = Cipher::aes_256_cbc();
    let encryption_iv = generate_random_iv();

    let mut encrypter = Crypter::new(cipher, Mode::Encrypt, KEY.as_bytes(), Some(&encryption_iv))?;

    let mut encrypted_data = vec![0u8; input.len() + cipher.block_size()];
    let mut encrypted_len = encrypter.update(input.as_bytes(), &mut encrypted_data)?;

    encrypted_len += encrypter.finalize(&mut encrypted_data[encrypted_len..])?;

    encrypted_data.truncate(encrypted_len);
    
    let mut result = encryption_iv.to_vec();
    result.extend_from_slice(&encrypted_data);

    Ok(general_purpose::STANDARD.encode(&result))
}

fn generate_random_iv() -> Vec<u8> {
    let mut rng = rand::thread_rng();
    let mut bytes = vec![0u8; IV_LENGTH];
    rng.fill(&mut bytes[..]);
    bytes
}
