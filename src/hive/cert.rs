use crate::hive::identity::DeviceIdentity;
use anyhow::{Context, Result};
use ed25519_dalek::pkcs8::EncodePrivateKey;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Converts this device's existing Ed25519 signing key into an `rcgen`
/// keypair, so the TLS certificate is a wrapper around the same identity
/// used for roster join/revocation records — not a second, unrelated key.
pub fn identity_to_rcgen_keypair(identity: &DeviceIdentity) -> Result<rcgen::KeyPair> {
    let pkcs8_der = identity
        .signing_key
        .to_pkcs8_der()
        .context("encoding device signing key as PKCS#8")?;
    let pkcs8_der = rustls_pki_types::PrivatePkcs8KeyDer::from(pkcs8_der.as_bytes().to_vec());
    rcgen::KeyPair::from_pkcs8_der_and_sign_algo(&pkcs8_der, &rcgen::PKCS_ED25519)
        .context("building rcgen KeyPair from device signing key")
}

/// A self-signed certificate for this device, using its own persisted
/// identity key. Only the *public key* inside it needs to be stable across
/// restarts, and it always is, because it's derived from the persisted
/// signing key every time.
pub fn self_signed_cert(identity: &DeviceIdentity) -> Result<rcgen::CertifiedKey<rcgen::KeyPair>> {
    let keypair = identity_to_rcgen_keypair(identity)?;
    let params = rcgen::CertificateParams::new(vec![identity.device_id.clone()])
        .context("building certificate params")?;
    let cert = params
        .self_signed(&keypair)
        .context("self-signing certificate")?;
    Ok(rcgen::CertifiedKey {
        cert,
        signing_key: keypair,
    })
}

/// Process-wide cache of `(certificate DER, PKCS#8 private-key DER)` keyed by
/// `device_id`. The self-signed cert is fully deterministic from the device
/// identity, which is fixed for the process lifetime — yet `self_signed_cert`
/// does non-trivial CPU work each call (PKCS#8 decode, rcgen keypair build,
/// ASN.1 cert construction, an Ed25519 self-signature). `HiveClient::new`
/// rebuilds a client per online peer on *every* memory write (push-on-change),
/// so recomputing the identical cert every time is pure waste. Memoize the DER
/// bytes once per identity; keying by `device_id` keeps multiple identities
/// (tests, and in principle any future multi-identity use) correct rather than
/// assuming a single process-global identity.
type CertDer = (Vec<u8>, Vec<u8>);
static CERT_DER_CACHE: OnceLock<Mutex<HashMap<String, CertDer>>> = OnceLock::new();

/// Returns the cached `(cert DER, PKCS#8 key DER)` for this identity,
/// computing and caching it on first use. This is what both the outbound
/// mTLS client and the inbound sync listener actually need — they only ever
/// pull these two byte vectors out of the `CertifiedKey`.
pub fn self_signed_cert_der(identity: &DeviceIdentity) -> Result<(Vec<u8>, Vec<u8>)> {
    let cache = CERT_DER_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(cached) = cache.lock().unwrap().get(&identity.device_id) {
        return Ok(cached.clone());
    }
    let certified = self_signed_cert(identity)?;
    let cert_der = certified.cert.der().to_vec();
    let key_der = certified.signing_key.serialize_der();
    cache.lock().unwrap().insert(
        identity.device_id.clone(),
        (cert_der.clone(), key_der.clone()),
    );
    Ok((cert_der, key_der))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hive::identity;

    #[test]
    fn cert_public_key_matches_identity_public_key() {
        let device_identity = identity::generate();
        let cert = self_signed_cert(&device_identity).unwrap();
        let der = cert.cert.der();
        let (_, parsed) = x509_parser::parse_x509_certificate(der).unwrap();
        let raw_pubkey = parsed.public_key().subject_public_key.data.as_ref();
        let expected = hex::decode(identity::public_key_hex(&device_identity)).unwrap();
        assert_eq!(raw_pubkey, expected.as_slice());
    }
}
