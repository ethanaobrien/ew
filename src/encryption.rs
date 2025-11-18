use base64::{Engine as _, engine::general_purpose};
use rand::Rng;
use aes::cipher::BlockEncryptMut;
use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

const IV_LENGTH: usize = 16;
const KEY: &str = "3559b435f24b297a79c68b9709ef2125";

pub fn decrypt_packet(base64_input: &str) -> Result<String, String> {
    if base64_input.len() < IV_LENGTH + 1 {
        return Ok(String::new());
    }
    let base64_buffer = general_purpose::STANDARD.decode(base64_input).unwrap();

    let decryption_iv = &base64_buffer[..IV_LENGTH];
    let mut ciphertext = base64_buffer[IV_LENGTH..].to_vec();

    let decrypted_data = Aes256CbcDec::new(KEY.as_bytes().into(), decryption_iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut ciphertext).ok()
        .ok_or(String::from("uhoh"))?;

    Ok(String::from_utf8(decrypted_data.to_vec()).unwrap())
}

pub fn encrypt_packet(input: &str) -> Result<String, String> {
    let encryption_iv = generate_random_iv();

    let encrypted = Aes256CbcEnc::new(KEY.as_bytes().into(), encryption_iv.as_slice().into())
        .encrypt_padded_vec_mut::<Pkcs7>(input.as_bytes());

    let mut result = encryption_iv.to_vec();
    result.extend_from_slice(&encrypted);

    Ok(general_purpose::STANDARD.encode(&result))
}

fn generate_random_iv() -> Vec<u8> {
    let mut rng = rand::rng();
    let mut bytes = vec![0u8; IV_LENGTH];
    rng.fill(&mut bytes[..]);
    bytes
}
