use std::collections::HashMap;

use crate::hive::roster::{
    RosterEntry, RosterStatus, verify_join_record, verify_revocation_record,
};

/// Merges an incoming roster (received from a peer over gossip) into the
/// local roster, applying the trust rules that make revocation
/// gossip-safe:
///
/// 1. Any incoming entry whose `join_record` doesn't verify is dropped
///    entirely.
/// 2. An incoming entry not yet known locally is added, forced `Active`
///    with no revocation state, regardless of whatever the incoming struct
///    itself claims -- a first-seen device_id can never be pre-poisoned as
///    `Revoked` without a verified revocation record, since this path never
///    checks one.
///    3./4. An incoming revocation for a locally-`Active` entry is applied only
///    if: the revocation record's own claimed target (`device_id`) matches
///    the candidate entry it's attached to (preventing cross-target
///    replay), AND it verifies against the revoker's public key AND the
///    revoker is itself a currently-`Active` entry in the local roster *as
///    it stood before this merge call began* (preventing same-batch trust
///    bootstrapping, where a throwaway device added earlier in this same
///    incoming batch is used as the "trusted" revoker later in the same
///    batch). This covers both self-revocation (revoker == target) and the
///    common case of a separate trusted revoker.
/// 5. A `Revoked` local entry is never overwritten back to `Active`.
/// 6. Merge is idempotent.
///
/// This is a pure function: no I/O, no clock reads. Callers (pairing
/// handshake, data-sync exchange) are responsible for persisting the result.
pub fn merge_roster(local: Vec<RosterEntry>, incoming: Vec<RosterEntry>) -> Vec<RosterEntry> {
    // Frozen BEFORE the merge loop mutates anything -- revoker trust is
    // checked against this snapshot only, never against `merged`'s
    // in-progress state.
    let locally_active_before_merge: HashMap<String, String> = local
        .iter()
        .filter(|e| e.status == RosterStatus::Active)
        .map(|e| (e.device_id.clone(), e.public_key.clone()))
        .collect();

    let mut merged = local;

    for candidate in incoming {
        if !verify_join_record(&candidate.join_record) {
            continue;
        }
        // Only `join_record` is signature-checked, but it's the *outer*
        // `device_id`/`public_key` that get persisted, that the sync
        // listener's client-cert verifier accepts, and that the
        // revoker-trust snapshot above keys on. Bind the two together:
        // an entry whose outer identity disagrees with its own verified
        // join record is dropped outright. Otherwise any Active peer could
        // gossip `{device_id: <victim's id>, public_key: <attacker key>,
        // join_record: <attacker's own valid record>}` and, on a device
        // that hasn't met the victim yet, squat the victim's id with an
        // attacker-controlled key -- the victim's real entry would then be
        // ignored forever (first-seen wins), and a revocation aimed at the
        // victim's id would hit the wrong key.
        if candidate.device_id != candidate.join_record.device_id
            || candidate.public_key != candidate.join_record.public_key
        {
            continue;
        }

        match merged
            .iter()
            .position(|e| e.device_id == candidate.device_id)
        {
            None => {
                // Rule 2: first-seen entries are always added Active,
                // regardless of whatever status/revocation fields the
                // incoming struct happens to carry. Name and join time are
                // likewise taken from the signed join record, not the
                // unsigned outer fields.
                merged.push(RosterEntry {
                    name: candidate.join_record.name.clone(),
                    joined_at: candidate.join_record.joined_at,
                    status: RosterStatus::Active,
                    revoked_at: None,
                    revoked_by: None,
                    revocation_record: None,
                    ..candidate
                });
            }
            Some(idx) => {
                if merged[idx].status == RosterStatus::Revoked {
                    continue; // sticky: never un-revoke
                }
                if let Some(revocation) = &candidate.revocation_record {
                    // The revocation record's own claimed target must
                    // match the entry we're about to apply it to --
                    // otherwise a legitimate revocation for one device
                    // could be replayed against an unrelated one.
                    if revocation.device_id != candidate.device_id {
                        continue;
                    }

                    let target_public_key = merged[idx].public_key.clone();

                    // Degenerate case: a device revoking itself, verified
                    // against ITS OWN public key.
                    let self_revoked = revocation.revoked_by == candidate.device_id
                        && verify_revocation_record(revocation, &target_public_key);

                    // Common case: revocation from a different member who
                    // was ALREADY active before this merge began.
                    let revoker_trusted = locally_active_before_merge
                        .get(&revocation.revoked_by)
                        .map(|revoker_pk| verify_revocation_record(revocation, revoker_pk))
                        .unwrap_or(false);

                    if self_revoked || revoker_trusted {
                        merged[idx].status = RosterStatus::Revoked;
                        merged[idx].revoked_at = Some(revocation.revoked_at);
                        merged[idx].revoked_by = Some(revocation.revoked_by.clone());
                        merged[idx].revocation_record = Some(revocation.clone());
                    }
                }
            }
        }
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hive::identity::{self, DeviceIdentity};
    use crate::hive::roster::create_join_record;
    use crate::hive::roster::create_revocation_record;

    fn entry_for(identity: &DeviceIdentity, name: &str, joined_at: i64) -> RosterEntry {
        let join_record = create_join_record(identity, name, joined_at);
        RosterEntry {
            device_id: identity.device_id.clone(),
            public_key: identity::public_key_hex(identity),
            name: name.to_string(),
            status: RosterStatus::Active,
            joined_at,
            revoked_at: None,
            revoked_by: None,
            join_record,
            revocation_record: None,
        }
    }

    #[test]
    fn adds_new_unknown_device() {
        let device = identity::generate();
        let entry = entry_for(&device, "alice-laptop", 1000);
        let merged = merge_roster(vec![], vec![entry.clone()]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].device_id, entry.device_id);
        assert_eq!(merged[0].status, RosterStatus::Active);
    }

    #[test]
    fn drops_entry_with_invalid_join_record() {
        let device = identity::generate();
        let mut entry = entry_for(&device, "alice-laptop", 1000);
        entry.join_record.name = "tampered".to_string();
        let merged = merge_roster(vec![], vec![entry]);
        assert!(merged.is_empty());
    }

    #[test]
    fn applies_revocation_from_active_local_revoker() {
        let a = identity::generate();
        let b = identity::generate();
        let local = vec![
            entry_for(&a, "alice-laptop", 1000),
            entry_for(&b, "bob-phone", 1100),
        ];

        let revocation = create_revocation_record(&a, &b.device_id, 2000);
        let mut incoming_b = entry_for(&b, "bob-phone", 1100);
        incoming_b.revocation_record = Some(revocation);

        let merged = merge_roster(local, vec![incoming_b]);
        let b_entry = merged.iter().find(|e| e.device_id == b.device_id).unwrap();
        assert_eq!(b_entry.status, RosterStatus::Revoked);
        assert_eq!(b_entry.revoked_by.as_deref(), Some(a.device_id.as_str()));
    }

    #[test]
    fn rejects_revocation_from_revoker_not_locally_active() {
        let a = identity::generate();
        let b = identity::generate();
        // "a" is NOT in the local roster at all, so it can't be trusted as a revoker.
        let local = vec![entry_for(&b, "bob-phone", 1100)];

        let revocation = create_revocation_record(&a, &b.device_id, 2000);
        let mut incoming_b = entry_for(&b, "bob-phone", 1100);
        incoming_b.revocation_record = Some(revocation);

        let merged = merge_roster(local, vec![incoming_b]);
        let b_entry = merged.iter().find(|e| e.device_id == b.device_id).unwrap();
        assert_eq!(
            b_entry.status,
            RosterStatus::Active,
            "revocation from an untrusted revoker must not apply"
        );
    }

    #[test]
    fn revocation_is_sticky_never_reverts_to_active() {
        let a = identity::generate();
        let b = identity::generate();
        let mut b_entry = entry_for(&b, "bob-phone", 1100);
        b_entry.status = RosterStatus::Revoked;
        b_entry.revoked_by = Some(a.device_id.clone());
        b_entry.revoked_at = Some(2000);
        let local = vec![entry_for(&a, "alice-laptop", 1000), b_entry];

        // A later incoming entry claims b is still active (e.g. b itself gossiping
        // its own unrevoked join record after being revoked) -- must not un-revoke.
        let incoming_b_active_again = entry_for(&b, "bob-phone", 1100);
        let merged = merge_roster(local, vec![incoming_b_active_again]);
        let b_final = merged.iter().find(|e| e.device_id == b.device_id).unwrap();
        assert_eq!(b_final.status, RosterStatus::Revoked);
    }

    #[test]
    fn merge_is_idempotent() {
        let device = identity::generate();
        let entry = entry_for(&device, "alice-laptop", 1000);
        let once = merge_roster(vec![], vec![entry.clone()]);
        let twice = merge_roster(once.clone(), vec![entry]);
        assert_eq!(once.len(), twice.len());
        assert_eq!(once[0].device_id, twice[0].device_id);
        assert_eq!(once[0].status, twice[0].status);
    }

    // Bug 1: same-batch trust bootstrapping. A throwaway device C is added
    // as Active by the same incoming batch that also carries a revocation
    // "from" C targeting a real, previously-active device B. C must not be
    // usable as a trusted revoker just because it appears earlier in the
    // same batch -- trust is checked against the roster as it stood BEFORE
    // this merge call began.
    #[test]
    fn rejects_same_batch_revoker_bootstrapping() {
        let b = identity::generate();
        let c = identity::generate();
        let local = vec![entry_for(&b, "bob-phone", 1100)];

        let c_join = entry_for(&c, "throwaway-c", 1500);

        let revocation = create_revocation_record(&c, &b.device_id, 2000);
        let mut incoming_b = entry_for(&b, "bob-phone", 1100);
        incoming_b.revocation_record = Some(revocation);

        let merged = merge_roster(local, vec![c_join, incoming_b]);
        let b_entry = merged.iter().find(|e| e.device_id == b.device_id).unwrap();
        assert_eq!(
            b_entry.status,
            RosterStatus::Active,
            "a device added earlier in the same batch must not be trusted as a revoker"
        );
    }

    // Bug 2: cross-target replay. A genuine revocation record for device D
    // (signed by a real, locally-trusted revoker A) gets reattached to a
    // different candidate entry for device B. It must be rejected because
    // the record's own claimed target (D) doesn't match the entry it's
    // attached to (B).
    #[test]
    fn rejects_cross_target_revocation_replay() {
        let a = identity::generate();
        let b = identity::generate();
        let d = identity::generate();
        let local = vec![
            entry_for(&a, "alice-laptop", 1000),
            entry_for(&b, "bob-phone", 1100),
            entry_for(&d, "dave-tablet", 1200),
        ];

        // Genuine revocation: A revokes D.
        let revocation_for_d = create_revocation_record(&a, &d.device_id, 2000);

        // Replayed onto B's own, otherwise-valid, unrelated entry.
        let mut incoming_b = entry_for(&b, "bob-phone", 1100);
        incoming_b.revocation_record = Some(revocation_for_d);

        let merged = merge_roster(local, vec![incoming_b]);
        let b_entry = merged.iter().find(|e| e.device_id == b.device_id).unwrap();
        assert_eq!(
            b_entry.status,
            RosterStatus::Active,
            "a revocation record for a different target must not apply to this entry"
        );
    }

    // Bug 3: a first-seen candidate with status forced to Revoked directly
    // (no verified revocation record at all) must be added as Active.
    #[test]
    fn rejects_pre_poisoned_new_entry() {
        let c = identity::generate();
        let mut poisoned = entry_for(&c, "poisoned-c", 1000);
        poisoned.status = RosterStatus::Revoked;
        poisoned.revoked_by = Some("nobody".to_string());
        poisoned.revoked_at = Some(999);

        let merged = merge_roster(vec![], vec![poisoned]);
        let c_entry = merged.iter().find(|e| e.device_id == c.device_id).unwrap();
        assert_eq!(
            c_entry.status,
            RosterStatus::Active,
            "a first-seen entry must never be added pre-poisoned as Revoked"
        );
        assert!(c_entry.revoked_by.is_none());
        assert!(c_entry.revoked_at.is_none());
        assert!(c_entry.revocation_record.is_none());
    }

    // Identity squatting: an otherwise-valid join record (the attacker's
    // own) wrapped in an entry whose outer public_key is a different key.
    // Without the outer/inner cross-check the entry would be persisted
    // under the outer key, which is what the TLS gate trusts.
    #[test]
    fn rejects_entry_whose_outer_public_key_disagrees_with_join_record() {
        let attacker = identity::generate();
        let other_key = identity::generate();
        let mut entry = entry_for(&attacker, "attacker", 1000);
        entry.public_key = identity::public_key_hex(&other_key);

        let merged = merge_roster(vec![], vec![entry]);
        assert!(
            merged.is_empty(),
            "an entry whose outer public_key isn't the one its join record was signed with must be dropped"
        );
    }

    #[test]
    fn rejects_entry_whose_outer_device_id_disagrees_with_join_record() {
        let attacker = identity::generate();
        let victim = identity::generate();
        // Valid join record for `attacker`, relabelled with the victim's id.
        let mut entry = entry_for(&attacker, "attacker", 1000);
        entry.device_id = victim.device_id.clone();

        let merged = merge_roster(vec![], vec![entry]);
        assert!(
            merged.is_empty(),
            "an entry squatting another device_id over a foreign join record must be dropped"
        );
    }

    #[test]
    fn new_entry_takes_name_and_joined_at_from_the_signed_join_record() {
        let device = identity::generate();
        let mut entry = entry_for(&device, "real-name", 1000);
        entry.name = "unsigned-outer-name".to_string();
        entry.joined_at = 999_999;

        let merged = merge_roster(vec![], vec![entry]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].name, "real-name");
        assert_eq!(merged[0].joined_at, 1000);
    }

    // Gap 1: self-revocation, where revoked_by == the target's own
    // device_id and the signature verifies against the target's own
    // public key (not some other revoker's).
    #[test]
    fn applies_self_revocation() {
        let b = identity::generate();
        let local = vec![entry_for(&b, "bob-phone", 1100)];

        let self_revocation = create_revocation_record(&b, &b.device_id, 2000);
        let mut incoming_b = entry_for(&b, "bob-phone", 1100);
        incoming_b.revocation_record = Some(self_revocation);

        let merged = merge_roster(local, vec![incoming_b]);
        let b_entry = merged.iter().find(|e| e.device_id == b.device_id).unwrap();
        assert_eq!(b_entry.status, RosterStatus::Revoked);
        assert_eq!(b_entry.revoked_by.as_deref(), Some(b.device_id.as_str()));
    }

    // Gap 2: idempotency of a revocation merge specifically -- merging the
    // same revoking batch twice must not change status/revoked_at/revoked_by
    // between the first and second merge.
    #[test]
    fn revocation_merge_is_idempotent() {
        let a = identity::generate();
        let b = identity::generate();
        let local = vec![
            entry_for(&a, "alice-laptop", 1000),
            entry_for(&b, "bob-phone", 1100),
        ];

        let revocation = create_revocation_record(&a, &b.device_id, 2000);
        let mut incoming_b = entry_for(&b, "bob-phone", 1100);
        incoming_b.revocation_record = Some(revocation);

        let once = merge_roster(local, vec![incoming_b.clone()]);
        let twice = merge_roster(once.clone(), vec![incoming_b]);

        let once_b = once.iter().find(|e| e.device_id == b.device_id).unwrap();
        let twice_b = twice.iter().find(|e| e.device_id == b.device_id).unwrap();

        assert_eq!(once_b.status, twice_b.status);
        assert_eq!(once_b.revoked_at, twice_b.revoked_at);
        assert_eq!(once_b.revoked_by, twice_b.revoked_by);
        assert_eq!(once_b.status, RosterStatus::Revoked);
    }
}
