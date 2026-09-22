use crate::hive::identity::{self, DeviceIdentity};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRecord {
    pub device_id: String,
    pub public_key: String,
    pub name: String,
    pub joined_at: i64,
    pub signature: String,
}

pub fn create_join_record(identity: &DeviceIdentity, name: &str, joined_at: i64) -> JoinRecord {
    let public_key = identity::public_key_hex(identity);
    let message = format!(
        "join:{}:{}:{}:{}",
        identity.device_id, public_key, name, joined_at
    );
    let signature = identity::sign(identity, message.as_bytes());
    JoinRecord {
        device_id: identity.device_id.clone(),
        public_key,
        name: name.to_string(),
        joined_at,
        signature,
    }
}

pub fn verify_join_record(record: &JoinRecord) -> bool {
    let message = format!(
        "join:{}:{}:{}:{}",
        record.device_id, record.public_key, record.name, record.joined_at
    );
    if !identity::verify(&record.public_key, message.as_bytes(), &record.signature) {
        return false;
    }
    let Some(expected_device_id) = identity::device_id_from_public_key_hex(&record.public_key)
    else {
        return false;
    };
    record.device_id == expected_device_id
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevocationRecord {
    pub device_id: String,
    pub revoked_by: String,
    pub revoked_at: i64,
    pub signature: String,
}

pub fn create_revocation_record(
    revoker: &DeviceIdentity,
    target_device_id: &str,
    revoked_at: i64,
) -> RevocationRecord {
    let message = format!(
        "revoke:{}:{}:{}",
        target_device_id, revoker.device_id, revoked_at
    );
    let signature = identity::sign(revoker, message.as_bytes());
    RevocationRecord {
        device_id: target_device_id.to_string(),
        revoked_by: revoker.device_id.clone(),
        revoked_at,
        signature,
    }
}

pub fn verify_revocation_record(record: &RevocationRecord, revoker_public_key: &str) -> bool {
    let message = format!(
        "revoke:{}:{}:{}",
        record.device_id, record.revoked_by, record.revoked_at
    );
    identity::verify(revoker_public_key, message.as_bytes(), &record.signature)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RosterStatus {
    Active,
    Revoked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RosterEntry {
    pub device_id: String,
    pub public_key: String,
    pub name: String,
    pub status: RosterStatus,
    pub joined_at: i64,
    pub revoked_at: Option<i64>,
    pub revoked_by: Option<String>,
    pub join_record: JoinRecord,
    pub revocation_record: Option<RevocationRecord>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hive::identity;

    #[test]
    fn join_record_round_trips() {
        let identity = identity::generate();
        let record = create_join_record(&identity, "alice-laptop", 1000);
        assert!(verify_join_record(&record));
    }

    #[test]
    fn join_record_rejects_tampered_name() {
        let identity = identity::generate();
        let mut record = create_join_record(&identity, "alice-laptop", 1000);
        record.name = "attacker-device".to_string();
        assert!(!verify_join_record(&record));
    }

    #[test]
    fn join_record_rejects_device_id_public_key_mismatch() {
        let identity = identity::generate();
        let public_key = identity::public_key_hex(&identity);
        let fake_device_id = "hive_00000000000000000000000000000000";
        let name = "alice-laptop";
        let joined_at = 1000i64;
        // Sign a message that is internally consistent with what verify_join_record
        // will reconstruct, EXCEPT the device_id doesn't match the public_key's
        // real derivation -- the signature itself is genuinely valid over these
        // exact (wrong) fields, isolating the device_id/public_key cross-check
        // from the signature check.
        let message = format!(
            "join:{}:{}:{}:{}",
            fake_device_id, public_key, name, joined_at
        );
        let signature = identity::sign(&identity, message.as_bytes());
        let record = JoinRecord {
            device_id: fake_device_id.to_string(),
            public_key,
            name: name.to_string(),
            joined_at,
            signature,
        };
        assert!(
            !verify_join_record(&record),
            "a validly-signed record with a device_id inconsistent with its own public_key must still be rejected"
        );
    }

    #[test]
    fn revocation_record_round_trips() {
        let revoker = identity::generate();
        let record = create_revocation_record(&revoker, "hive_targetdeviceid00", 2000);
        let revoker_pk = identity::public_key_hex(&revoker);
        assert!(verify_revocation_record(&record, &revoker_pk));
    }

    #[test]
    fn revocation_record_rejects_wrong_revoker_key() {
        let revoker = identity::generate();
        let impostor = identity::generate();
        let record = create_revocation_record(&revoker, "hive_targetdeviceid00", 2000);
        let impostor_pk = identity::public_key_hex(&impostor);
        assert!(!verify_revocation_record(&record, &impostor_pk));
    }
}
