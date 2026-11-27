//! Canonical records (signed or authenticated bytes) and the safety number.

use super::util::{labeled, sha256};

pub(super) fn key_record(user_id: u64, identity_pub: &[u8; 32], enc_pub: &[u8; 32]) -> Vec<u8> {
    labeled("keys", &[&user_id.to_be_bytes(), identity_pub, enc_pub])
}

/// `kind ‖ a ‖ b` (spec §4).
pub(super) type ConvDesc = [u8; 17];

pub(super) fn conv_desc(kind: u8, a: u64, b: u64) -> ConvDesc {
    let mut out = [0; 17];
    out[0] = kind;
    out[1..9].copy_from_slice(&a.to_be_bytes());
    out[9..].copy_from_slice(&b.to_be_bytes());
    out
}

pub(super) fn epoch_record(
    conversation_id: u64,
    desc: &ConvDesc,
    epoch: u32,
    creator_id: u64,
    creator_key_id: u64,
    key_commit: &[u8; 32],
) -> Vec<u8> {
    labeled(
        "epoch",
        &[
            &conversation_id.to_be_bytes(),
            desc,
            &epoch.to_be_bytes(),
            &creator_id.to_be_bytes(),
            &creator_key_id.to_be_bytes(),
            key_commit,
        ],
    )
}

pub(super) struct WrapHead<'a> {
    pub conversation_id: u64,
    pub conv_desc: &'a ConvDesc,
    pub epoch: u32,
    pub sender_id: u64,
    pub sender_key_id: u64,
    pub recipient_id: u64,
    pub recipient_key_id: u64,
    pub recipient_enc_pub: &'a [u8; 32],
    pub key_commit: &'a [u8; 32],
}

impl WrapHead<'_> {
    pub(super) fn bytes(&self) -> Vec<u8> {
        labeled(
            "wrap",
            &[
                &self.conversation_id.to_be_bytes(),
                self.conv_desc,
                &self.epoch.to_be_bytes(),
                &self.sender_id.to_be_bytes(),
                &self.sender_key_id.to_be_bytes(),
                &self.recipient_id.to_be_bytes(),
                &self.recipient_key_id.to_be_bytes(),
                self.recipient_enc_pub,
                self.key_commit,
            ],
        )
    }
}

pub(super) fn message_aad(
    conversation_id: u64,
    sender_id: u64,
    sender_key_id: u64,
    msg_id: &[u8; 16],
    revision: u32,
    epoch: u32,
) -> Vec<u8> {
    labeled(
        "aad",
        &[
            &conversation_id.to_be_bytes(),
            &sender_id.to_be_bytes(),
            &sender_key_id.to_be_bytes(),
            msg_id,
            &revision.to_be_bytes(),
            &epoch.to_be_bytes(),
        ],
    )
}

/// The same for both users, whoever computes it: hex in groups of four.
pub fn safety_number(a_id: u64, a_identity: &[u8; 32], b_id: u64, b_identity: &[u8; 32]) -> String {
    let mut users = [(a_id, a_identity), (b_id, b_identity)];
    users.sort();
    let [(lo, lo_key), (hi, hi_key)] = users;
    let hash = sha256(&labeled(
        "safety",
        &[&lo.to_be_bytes(), lo_key, &hi.to_be_bytes(), hi_key],
    ));
    hash.chunks(2)
        .map(|c| format!("{:02x}{:02x}", c[0], c[1]))
        .collect::<Vec<_>>()
        .join(" ")
}
