//! SQLCipher 4's page encryption with its default parameters, which is what
//! rekordbox writes `exportLibrary.db` with: AES-256-CBC with a random IV per
//! page, an HMAC-SHA512 per page, keys from PBKDF2-HMAC-SHA512 over a random
//! salt that takes the place of the "SQLite format 3" header. Applied to a
//! plain SQLite file whose pages keep [`RESERVE`] bytes free, it gives what
//! SQLCipher itself writes, without SQLCipher and the OpenSSL it needs outside
//! Apple, which doubled the size of the Linux and Windows binaries (#256).
//! Checked on every page and WAL frame of rekordbox's own file.

use aes::Aes256;
use anyhow::{ensure, Context};
use cbc::cipher::block_padding::NoPadding;
use cbc::cipher::{BlockModeEncrypt, KeyIvInit};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha512;

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
        let cipher = pbkdf2::pbkdf2_hmac_array::<Sha512, 32>(passphrase, salt, 256_000);
        let hmac_salt: Vec<u8> = salt.iter().map(|b| b ^ 0x3a).collect();
        let hmac = pbkdf2::pbkdf2_hmac_array::<Sha512, 32>(&cipher, &hmac_salt, 2);
        Keys { cipher, hmac }
    }

    /// Over a page's ciphertext and IV, then its number little-endian.
    fn mac(&self, data: &[u8], pgno: u32) -> [u8; MAC] {
        let mut m = <Hmac<Sha512> as KeyInit>::new_from_slice(&self.hmac).expect("any key length");
        m.update(data);
        m.update(&pgno.to_le_bytes());
        m.finalize().into_bytes().into()
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
        let len = data.len();
        cbc::Encryptor::<Aes256>::new_from_slices(&keys.cipher, iv)
            .expect("key and IV lengths")
            .encrypt_padded::<NoPadding>(data, len)
            .expect("whole blocks");
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
    use cbc::cipher::BlockModeDecrypt;

    let keys = Keys::derive(passphrase, &db[..SALT]);
    let open = |page: &mut [u8], pgno: u32| {
        let start = start(pgno);
        assert!(
            keys.mac(&page[start..PAGE - MAC], pgno) == page[PAGE - MAC..],
            "HMAC of page {pgno}"
        );
        let (data, tail) = page[start..].split_at_mut(PAGE - start - RESERVE);
        cbc::Decryptor::<Aes256>::new_from_slices(&keys.cipher, &tail[..IV])
            .unwrap()
            .decrypt_padded::<NoPadding>(data)
            .unwrap();
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
