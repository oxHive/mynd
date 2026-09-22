use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};

#[derive(Clone)]
pub struct DeviceIdentity {
    pub device_id: String,
    pub signing_key: SigningKey,
}

pub fn generate() -> DeviceIdentity {
    let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
    let device_id = device_id_from_public_key(&signing_key.verifying_key());
    DeviceIdentity {
        device_id,
        signing_key,
    }
}

fn device_id_from_public_key(vk: &VerifyingKey) -> String {
    format!("hive_{}", hex::encode(&vk.to_bytes()[..16]))
}

/// Derives the device id from a hex-encoded public key, returning `None` if
/// the hex string cannot be decoded into a valid public key. This is the
/// single source of truth for the device_id/public_key relationship shared
/// with callers (e.g. `roster::verify_join_record`) that only have the
/// public key in hex form.
pub(crate) fn device_id_from_public_key_hex(public_key_hex: &str) -> Option<String> {
    let bytes = hex::decode(public_key_hex).ok()?;
    let bytes: [u8; 32] = bytes.try_into().ok()?;
    let vk = VerifyingKey::from_bytes(&bytes).ok()?;
    Some(device_id_from_public_key(&vk))
}

pub fn public_key_hex(identity: &DeviceIdentity) -> String {
    hex::encode(identity.signing_key.verifying_key().to_bytes())
}

/// Reconstructs a `DeviceIdentity` from a signing key previously persisted
/// via `public_key_hex`'s sibling save path (hex-encoded 32-byte seed).
/// Returns `None` if the hex is malformed or not exactly 32 bytes.
pub fn from_signing_key_hex(signing_key_hex: &str) -> Option<DeviceIdentity> {
    let bytes = hex::decode(signing_key_hex).ok()?;
    let bytes: [u8; 32] = bytes.try_into().ok()?;
    let signing_key = SigningKey::from_bytes(&bytes);
    let device_id = device_id_from_public_key(&signing_key.verifying_key());
    Some(DeviceIdentity {
        device_id,
        signing_key,
    })
}

pub fn sign(identity: &DeviceIdentity, message: &[u8]) -> String {
    hex::encode(identity.signing_key.sign(message).to_bytes())
}

pub fn verify(public_key_hex: &str, message: &[u8], signature_hex: &str) -> bool {
    let Ok(pk_bytes) = hex::decode(public_key_hex) else {
        return false;
    };
    let Ok(pk_bytes): Result<[u8; 32], _> = pk_bytes.try_into() else {
        return false;
    };
    let Ok(verifying_key) = VerifyingKey::from_bytes(&pk_bytes) else {
        return false;
    };
    let Ok(sig_bytes) = hex::decode(signature_hex) else {
        return false;
    };
    let Ok(sig_bytes): Result<[u8; 64], _> = sig_bytes.try_into() else {
        return false;
    };
    let signature = ed25519_dalek::Signature::from_bytes(&sig_bytes);
    verifying_key.verify(message, &signature).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_device_id_prefixed_hive() {
        let identity = generate();
        assert!(identity.device_id.starts_with("hive_"));
        assert_eq!(identity.device_id.len(), "hive_".len() + 32);
    }

    #[test]
    fn device_id_is_deterministic_from_public_key() {
        let identity = generate();
        let recomputed = device_id_from_public_key(&identity.signing_key.verifying_key());
        assert_eq!(identity.device_id, recomputed);
    }

    #[test]
    fn sign_then_verify_round_trips() {
        let identity = generate();
        let msg = b"join:hive_abc123";
        let sig = sign(&identity, msg);
        let pk = public_key_hex(&identity);
        assert!(verify(&pk, msg, &sig));
    }

    #[test]
    fn verify_rejects_tampered_message() {
        let identity = generate();
        let sig = sign(&identity, b"original message");
        let pk = public_key_hex(&identity);
        assert!(!verify(&pk, b"tampered message", &sig));
    }

    #[test]
    fn verify_rejects_wrong_public_key() {
        let identity = generate();
        let other = generate();
        let msg = b"some message";
        let sig = sign(&identity, msg);
        let wrong_pk = public_key_hex(&other);
        assert!(!verify(&wrong_pk, msg, &sig));
    }

    #[test]
    fn verify_rejects_malformed_hex() {
        assert!(!verify("not-hex", b"msg", "also-not-hex"));
    }

    #[test]
    fn from_signing_key_hex_round_trips_with_generate() {
        let identity = generate();
        let hex_key = hex::encode(identity.signing_key.to_bytes());
        let reconstructed = from_signing_key_hex(&hex_key).unwrap();
        assert_eq!(reconstructed.device_id, identity.device_id);
        assert_eq!(public_key_hex(&reconstructed), public_key_hex(&identity));
    }

    #[test]
    fn from_signing_key_hex_rejects_malformed_hex() {
        assert!(from_signing_key_hex("not-hex").is_none());
        assert!(from_signing_key_hex("deadbeef").is_none()); // too short
    }
}
