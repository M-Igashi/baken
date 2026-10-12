//! SQLCipher 4's page encryption with its default parameters, which is what
//! rekordbox writes `exportLibrary.db` with: AES-256-CBC with a random IV per
//! page, an HMAC-SHA512 per page, keys from PBKDF2-HMAC-SHA512 over a random
//! salt that takes the place of the "SQLite format 3" header. Applied to a
//! plain SQLite file whose pages keep [`RESERVE`] bytes free, it gives what
//! SQLCipher itself writes, without SQLCipher and the OpenSSL it needs outside
//! Apple, which doubled the size of the Linux and Windows binaries (#256).
//! Checked on every page and WAL frame of rekordbox's own file.
//!
//! On Apple targets the primitives come from CommonCrypto, part of the OS, so
//! the Mac app carries no cipher of its own and its export-compliance answer
//! stays "encryption within the Apple operating system" (baken-mac#83).
//! Elsewhere they come from RustCrypto.

use anyhow::{ensure, Context};

const PAGE: usize = 4096;
/// The tail of every page: its IV, then its HMAC.
pub const RESERVE: usize = IV + MAC;
const IV: usize = 16;
const MAC: usize = 64;
const SALT: usize = 16;
const HEADER: &[u8; SALT] = b"SQLite format 3\0";

struct Keys {
    cipher: [u8; 32],
    hmac: [u8; 32],
}

impl Keys {
    fn derive(passphrase: &[u8], salt: &[u8]) -> Self {
        let cipher = crypto::pbkdf2_sha512(passphrase, salt, 256_000);
        let hmac_salt: Vec<u8> = salt.iter().map(|b| b ^ 0x3a).collect();
        let hmac = crypto::pbkdf2_sha512(&cipher, &hmac_salt, 2);
        Keys { cipher, hmac }
    }

    /// Over a page's ciphertext and IV, then its number little-endian.
    fn mac(&self, data: &[u8], pgno: u32) -> [u8; MAC] {
        crypto::hmac_sha512(&self.hmac, &[data, &pgno.to_le_bytes()])
    }
}

/// Page 1 starts with the salt, so its ciphertext starts after it.
fn start(pgno: u32) -> usize {
    if pgno == 1 {
        SALT
    } else {
        0
    }
}

/// Encrypts the plain SQLite database `db` in place.
pub fn encrypt(db: &mut [u8], passphrase: &[u8]) -> anyhow::Result<()> {
    ensure!(
        db.starts_with(HEADER)
            && db.len().is_multiple_of(PAGE)
            && db[16..18] == (PAGE as u16).to_be_bytes()
            && usize::from(db[20]) == RESERVE,
        "not a database of {PAGE}-byte pages with {RESERVE} bytes reserved"
    );
    let mut salt = [0; SALT];
    getrandom::fill(&mut salt).context("random bytes")?;
    let keys = Keys::derive(passphrase, &salt);
    for (i, page) in db.chunks_exact_mut(PAGE).enumerate() {
        let pgno = i as u32 + 1;
        let start = start(pgno);
        let (data, tail) = page[start..].split_at_mut(PAGE - start - RESERVE);
        let iv = &mut tail[..IV];
        getrandom::fill(iv).context("random bytes")?;
        crypto::encrypt_cbc(&keys.cipher, iv, data);
        let mac = keys.mac(&page[start..PAGE - MAC], pgno);
        page[PAGE - MAC..].copy_from_slice(&mac);
    }
    db[..SALT].copy_from_slice(&salt);
    Ok(())
}

/// The plain database SQLCipher shows for `db` and its `-wal`: every page and
/// frame checked against its HMAC and decrypted, committed frames applied.
#[cfg(test)]
pub fn decrypt(db: &[u8], wal: &[u8], passphrase: &[u8]) -> Vec<u8> {
    let keys = Keys::derive(passphrase, &db[..SALT]);
    let open = |page: &mut [u8], pgno: u32| {
        let start = start(pgno);
        assert!(
            keys.mac(&page[start..PAGE - MAC], pgno) == page[PAGE - MAC..],
            "HMAC of page {pgno}"
        );
        let (data, tail) = page[start..].split_at_mut(PAGE - start - RESERVE);
        crypto::decrypt_cbc(&keys.cipher, &tail[..IV], data);
    };
    let mut plain = db.to_vec();
    for (i, page) in plain.chunks_exact_mut(PAGE).enumerate() {
        open(page, i as u32 + 1);
    }
    // A 32-byte header, then frames of a 24-byte header and a page; a frame
    // belongs to the log while it carries the header's salt.
    let be = |b: &[u8]| u32::from_be_bytes(b[..4].try_into().unwrap());
    let mut pending = Vec::new();
    for frame in wal.get(32..).unwrap_or_default().chunks_exact(24 + PAGE) {
        if frame[8..16] != wal[16..24] {
            break;
        }
        let pgno = be(frame);
        let mut page = frame[24..].to_vec();
        open(&mut page, pgno);
        pending.push((pgno, page));
        let pages = be(&frame[4..]) as usize;
        if pages > 0 {
            plain.resize(pages * PAGE, 0);
            for (pgno, page) in pending.drain(..) {
                let at = (pgno as usize - 1) * PAGE;
                plain[at..at + PAGE].copy_from_slice(&page);
            }
        }
    }
    plain[..SALT].copy_from_slice(HEADER);
    plain
}

/// AES-256-CBC without padding, in place, and PBKDF2 and HMAC over SHA-512.
#[cfg(target_vendor = "apple")]
mod crypto {
    use std::ffi::{c_int, c_void};

    const PBKDF2: u32 = 2;
    const PRF_HMAC_SHA512: u32 = 5;
    const HMAC_SHA512: u32 = 4;
    const ENCRYPT: u32 = 0;
    #[cfg(test)]
    const DECRYPT: u32 = 1;
    const AES: u32 = 0;
    /// Neither PKCS#7 padding nor ECB: CBC on whole blocks.
    const CBC_NO_PADDING: u32 = 0;

    #[repr(C)]
    struct HmacContext([u32; 96]);

    extern "C" {
        fn CCKeyDerivationPBKDF(
            algorithm: u32,
            password: *const u8,
            password_len: usize,
            salt: *const u8,
            salt_len: usize,
            prf: u32,
            rounds: u32,
            derived_key: *mut u8,
            derived_key_len: usize,
        ) -> c_int;
        fn CCHmacInit(ctx: *mut HmacContext, algorithm: u32, key: *const c_void, key_len: usize);
        fn CCHmacUpdate(ctx: *mut HmacContext, data: *const c_void, data_len: usize);
        fn CCHmacFinal(ctx: *mut HmacContext, mac_out: *mut c_void);
        fn CCCrypt(
            op: u32,
            alg: u32,
            options: u32,
            key: *const c_void,
            key_len: usize,
            iv: *const c_void,
            data_in: *const c_void,
            data_in_len: usize,
            data_out: *mut c_void,
            data_out_available: usize,
            data_out_moved: *mut usize,
        ) -> i32;
    }

    pub fn pbkdf2_sha512(password: &[u8], salt: &[u8], rounds: u32) -> [u8; 32] {
        let mut key = [0; 32];
        // SAFETY: every pointer comes with the length of its slice.
        let rc = unsafe {
            CCKeyDerivationPBKDF(
                PBKDF2,
                password.as_ptr(),
                password.len(),
                salt.as_ptr(),
                salt.len(),
                PRF_HMAC_SHA512,
                rounds,
                key.as_mut_ptr(),
                key.len(),
            )
        };
        assert_eq!(rc, 0, "CCKeyDerivationPBKDF");
        key
    }

    pub fn hmac_sha512(key: &[u8], parts: &[&[u8]]) -> [u8; 64] {
        let mut ctx = HmacContext([0; 96]);
        let mut mac = [0; 64];
        // SAFETY: the context outlives the calls, every pointer comes with
        // the length of its slice, and SHA-512 writes 64 bytes.
        unsafe {
            CCHmacInit(&mut ctx, HMAC_SHA512, key.as_ptr().cast(), key.len());
            for part in parts {
                CCHmacUpdate(&mut ctx, part.as_ptr().cast(), part.len());
            }
            CCHmacFinal(&mut ctx, mac.as_mut_ptr().cast());
        }
        mac
    }

    fn cbc(op: u32, key: &[u8; 32], iv: &[u8], data: &mut [u8]) {
        assert!(
            iv.len() == 16 && data.len().is_multiple_of(16),
            "IV and whole blocks"
        );
        let mut moved = 0;
        // SAFETY: CommonCrypto works in place when input and output are the
        // same buffer; the key, the IV and the data are as long as given.
        let rc = unsafe {
            CCCrypt(
                op,
                AES,
                CBC_NO_PADDING,
                key.as_ptr().cast(),
                key.len(),
                iv.as_ptr().cast(),
                data.as_ptr().cast(),
                data.len(),
                data.as_mut_ptr().cast(),
                data.len(),
                &mut moved,
            )
        };
        assert!(rc == 0 && moved == data.len(), "CCCrypt");
    }

    pub fn encrypt_cbc(key: &[u8; 32], iv: &[u8], data: &mut [u8]) {
        cbc(ENCRYPT, key, iv, data);
    }

    #[cfg(test)]
    pub fn decrypt_cbc(key: &[u8; 32], iv: &[u8], data: &mut [u8]) {
        cbc(DECRYPT, key, iv, data);
    }
}

#[cfg(not(target_vendor = "apple"))]
mod crypto {
    use aes::Aes256;
    use cbc::cipher::block_padding::NoPadding;
    use cbc::cipher::{BlockModeEncrypt, KeyIvInit};
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha512;

    pub fn pbkdf2_sha512(password: &[u8], salt: &[u8], rounds: u32) -> [u8; 32] {
        pbkdf2::pbkdf2_hmac_array::<Sha512, 32>(password, salt, rounds)
    }

    pub fn hmac_sha512(key: &[u8], parts: &[&[u8]]) -> [u8; 64] {
        let mut m = <Hmac<Sha512> as KeyInit>::new_from_slice(key).expect("any key length");
        for part in parts {
            m.update(part);
        }
        m.finalize().into_bytes().into()
    }

    pub fn encrypt_cbc(key: &[u8; 32], iv: &[u8], data: &mut [u8]) {
        let len = data.len();
        cbc::Encryptor::<Aes256>::new_from_slices(key, iv)
            .expect("key and IV lengths")
            .encrypt_padded::<NoPadding>(data, len)
            .expect("whole blocks");
    }

    #[cfg(test)]
    pub fn decrypt_cbc(key: &[u8; 32], iv: &[u8], data: &mut [u8]) {
        use cbc::cipher::BlockModeDecrypt;

        cbc::Decryptor::<Aes256>::new_from_slices(key, iv)
            .expect("key and IV lengths")
            .decrypt_padded::<NoPadding>(data)
            .expect("whole blocks");
    }
}

#[cfg(test)]
mod tests {
    use super::crypto;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Either backend against Python's hashlib and the OpenSSL command line.
    #[test]
    fn primitives_match_known_answers() {
        assert_eq!(
            crypto::pbkdf2_sha512(b"password", b"salt", 4096).to_vec(),
            hex("d197b1b33db0143e018b12f3d1d1479e6cdebdcc97c5c0f87f6902e072f457b5")
        );
        let key: [u8; 32] = std::array::from_fn(|i| i as u8);
        assert_eq!(
            crypto::hmac_sha512(&key, &[b"page data".as_slice(), &7u32.to_le_bytes()]).to_vec(),
            hex("7ff28f0d0b60e4fff4991d1c6089d23c1f0a30faee9ea57c1dd8984d4c868a43c3acfde5876765eb63c0c54aea8ebc22048e92b057b2af0001fef1548d11af75")
        );
        let iv: Vec<u8> = (16..32).collect();
        let plain: Vec<u8> = (0..64).collect();
        let mut data = plain.clone();
        crypto::encrypt_cbc(&key, &iv, &mut data);
        assert_eq!(
            data,
            hex("9f3b7504926f8bd36e3118e903a4cd4a25c183f70fdb4812cc2453fa00b3d390fbd621f919e2d9a0cb10aa5647bee3a7038906cdf1b7b2f800aa3b55e1b639f8")
        );
        crypto::decrypt_cbc(&key, &iv, &mut data);
        assert_eq!(data, plain);
    }
}
