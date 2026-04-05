// AES-256-GCM encryption with Argon2id key derivation.
//
// Format: [16-byte salt] [12-byte nonce] [encrypted_data + 16-byte auth_tag]
// Key derivation: Argon2id(password, salt) -> 32-byte key
// Encryption: AES-256-GCM(key, nonce, data) -> ciphertext + tag

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::Argon2;
use rand::RngCore;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

fn derive_key(password: &[u8], salt: &[u8]) -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    Argon2::default()
        .hash_password_into(password, salt, &mut key)
        .expect("argon2 key derivation failed");
    key
}

pub fn encrypt(data: &[u8], password: &str) -> Vec<u8> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let key = derive_key(password.as_bytes(), &salt);
    let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher.encrypt(nonce, data).expect("encryption failed");

    let mut out = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    out
}

pub fn decrypt(data: &[u8], password: &str) -> Result<Vec<u8>, &'static str> {
    if data.len() < SALT_LEN + NONCE_LEN + 16 {
        return Err("data too short");
    }

    let salt = &data[..SALT_LEN];
    let nonce_bytes = &data[SALT_LEN..SALT_LEN + NONCE_LEN];
    let ciphertext = &data[SALT_LEN + NONCE_LEN..];

    let key = derive_key(password.as_bytes(), salt);
    let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
    let nonce = Nonce::from_slice(nonce_bytes);

    cipher.decrypt(nonce, ciphertext).map_err(|_| "wrong password or corrupted data")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let data = b"secret data that must be protected";
        let enc = encrypt(data, "mypassword123");
        let dec = decrypt(&enc, "mypassword123").unwrap();
        assert_eq!(&dec[..], &data[..]);
    }

    #[test]
    fn wrong_password_fails() {
        let enc = encrypt(b"secret", "correct");
        assert!(decrypt(&enc, "wrong").is_err());
    }

    #[test]
    fn different_encryptions_differ() {
        let data = b"same data";
        let e1 = encrypt(data, "pass");
        let e2 = encrypt(data, "pass");
        assert_ne!(e1, e2); // different salt/nonce each time
    }
}
