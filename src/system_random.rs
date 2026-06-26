use rand::{CryptoRng, RngCore, TryRngCore, rngs::OsRng};

pub fn rng() -> impl RngCore + CryptoRng {
    OsRng.unwrap_err()
}

pub fn fill_bytes(bytes: &mut [u8]) {
    rng().fill_bytes(bytes);
}
