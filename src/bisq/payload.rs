mod custom_messages;

include!("../generated/io.bisq.protobuffer.rs");
include!("../generated/payload_macros.rs");

pub mod kind;

use super::{constants::*, hash::*};
use crate::prelude::{ripemd160, sha256, Hash};
use openssl::{dsa::Dsa, pkey::*, sign::Verifier};
use rand::{thread_rng, Rng};
use std::{
    fmt, io,
    net::{SocketAddr, ToSocketAddrs},
    str::FromStr,
    vec,
};

pub fn gen_nonce() -> i32 {
    thread_rng().gen()
}

impl ToSocketAddrs for NodeAddress {
    type Iter = vec::IntoIter<SocketAddr>;
    fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
        (&*self.host_name, self.port as u16).to_socket_addrs()
    }
}
impl FromStr for NodeAddress {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut iter = s.split(':');
        match (iter.next(), iter.next()) {
            (Some(host_name), Some(port)) if u16::from_str(&port).is_ok() => Ok(Self {
                host_name: host_name.to_string(),
                port: u16::from_str(&port).unwrap() as i32,
            }),
            (_, Some(_)) => Err("Couldn't parse port".to_string()),
            _ => Err("Couldn't parse node address".to_string()),
        }
    }
}
impl fmt::Display for NodeAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host_name, self.port)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MessageVersion(i32);
impl From<MessageVersion> for i32 {
    fn from(msg: MessageVersion) -> i32 {
        msg.0
    }
}
impl From<BaseCurrencyNetwork> for MessageVersion {
    fn from(network: BaseCurrencyNetwork) -> MessageVersion {
        MessageVersion((network as i32) + 10 * P2P_NETWORK_VERSION)
    }
}

impl StoragePayload {
    pub fn bisq_hash(&self) -> SequencedMessageHash {
        SequencedMessageHash::new(self.sha256())
    }

    fn signing_pub_key_bytes(&self) -> Option<&Vec<u8>> {
        match self.message.as_ref()? {
            storage_payload::Message::Alert(alert) => &alert.owner_pub_key_bytes,
            storage_payload::Message::Arbitrator(arb) => {
                &arb.pub_key_ring.as_ref()?.signature_pub_key_bytes
            }

            storage_payload::Message::Mediator(med) => {
                &med.pub_key_ring.as_ref()?.signature_pub_key_bytes
            }
            storage_payload::Message::Filter(filter) => &filter.owner_pub_key_bytes,
            storage_payload::Message::TradeStatistics(trade) => &trade.signature_pub_key_bytes,
            storage_payload::Message::MailboxStoragePayload(payload) => {
                &payload.sender_pub_key_for_add_operation_bytes
            }
            storage_payload::Message::OfferPayload(offer) => {
                &offer.pub_key_ring.as_ref()?.signature_pub_key_bytes
            }
            storage_payload::Message::TempProposalPayload(payload) => {
                &payload.owner_pub_key_encoded
            }
            storage_payload::Message::RefundAgent(agent) => {
                &agent.pub_key_ring.as_ref()?.signature_pub_key_bytes
            }
        }
        .into()
    }
}
impl ProtectedStorageEntry {
    fn owner_pub_key(&self) -> Option<PKey<Public>> {
        PKey::from_dsa(Dsa::public_key_from_der(&self.owner_pub_key_bytes).ok()?).ok()
    }
    pub fn verify(&self) -> Option<SequencedMessageHash> {
        let payload = self.storage_payload.as_ref()?;
        if payload.signing_pub_key_bytes()? != &self.owner_pub_key_bytes {
            warn!("Invalid public key in ProtectedStorageEntry");
            return None;
        }
        let pub_key = self.owner_pub_key()?;
        let verifier = Verifier::new_without_digest(&pub_key).ok()?;
        let hash = DataAndSeqNrPair {
            payload: Some(payload.clone()),
            sequence_number: self.sequence_number,
        }
        .sha256();
        verifier
            .verify_oneshot(&self.signature, &hash.into_inner())
            .ok()
            .and_then(|verified| {
                if verified {
                    Some(payload.bisq_hash())
                } else {
                    warn!(
                        "Detected invalid signature in ProtectedStorageEntry {:?}",
                        payload.bisq_hash()
                    );
                    None
                }
            })
    }
}
impl RefreshOfferMessage {
    pub fn payload_hash(&self) -> SequencedMessageHash {
        SequencedMessageHash::new(
            sha256::Hash::from_slice(&self.hash_of_payload)
                .expect("Couldn't unwrap RefreshOfferMessage.hash_of_data"),
        )
    }
    pub fn verify(&self, owner_pub_key: &[u8], original_payload: &StoragePayload) -> Option<()> {
        let hash = DataAndSeqNrPair {
            payload: Some(original_payload.clone()),
            sequence_number: self.sequence_number,
        }
        .sha256();
        if hash.into_inner() != *self.hash_of_data_and_seq_nr {
            warn!("Error with RefreshOfferMessage.hash_of_data_and_seq_nr");
            return None;
        }
        let pub_key = PKey::from_dsa(Dsa::public_key_from_der(owner_pub_key).ok()?).ok()?;
        let verifier = Verifier::new_without_digest(&pub_key).ok()?;
        verifier
            .verify_oneshot(&self.signature, &hash.into_inner())
            .ok()
            .and_then(|verified| {
                if verified {
                    Some(())
                } else {
                    warn!(
                        "Detected invalid signature in RefreshOfferMessage {:?}",
                        self.payload_hash()
                    );
                    None
                }
            })
    }
}

impl PersistableNetworkPayload {
    pub fn bisq_hash(&self) -> PersistentMessageHash {
        let inner = match self
            .message
            .as_ref()
            .expect("PersistableNetworkPayload doesn't have message attached")
        {
            persistable_network_payload::Message::AccountAgeWitness(witness) => {
                ripemd160::Hash::from_slice(&witness.hash)
                    .expect("AccountAgeWitness.hash is not correct")
            }
            persistable_network_payload::Message::TradeStatistics2(stats) => {
                ripemd160::Hash::from_slice(&stats.hash)
                    .expect("TradeStatistics2.hash is not correct")
            }
            persistable_network_payload::Message::ProposalPayload(prop) => {
                ripemd160::Hash::from_slice(&prop.hash)
                    .expect("ProposalPayload.hash is not correct")
            }
            persistable_network_payload::Message::BlindVotePayload(vote) => {
                ripemd160::Hash::from_slice(&vote.hash)
                    .expect("BlindVotePayload.hash is not correct")
            }
            persistable_network_payload::Message::SignedWitness(witness) => {
                let mut data = witness.account_age_witness_hash.clone();
                data.extend_from_slice(&witness.signature);
                data.extend_from_slice(&witness.signer_pub_key);
                let hash = sha256::Hash::hash(&data);
                ripemd160::Hash::hash(&hash.into_inner())
            }
        };
        PersistentMessageHash::new(inner)
    }
}

macro_rules! into_message {
    ($caml:ident, $snake:ident) => {
        impl From<$caml> for network_envelope::Message {
            fn from(msg: $caml) -> network_envelope::Message {
                network_envelope::Message::$caml(msg)
            }
        }
    };
}
for_all_payloads!(into_message);

pub enum Extract<P> {
    Succeeded(P),
    Failed(network_envelope::Message),
}
pub trait PayloadExtractor {
    type Extraction: Send;
    fn extract(msg: network_envelope::Message) -> Extract<Self::Extraction>;
}

macro_rules! extractor {
    ($caml:ident, $snake:ident) => {
        impl PayloadExtractor for $caml {
            type Extraction = $caml;
            fn extract(msg: network_envelope::Message) -> Extract<Self::Extraction> {
                if let network_envelope::Message::$caml(request) = msg {
                    Extract::Succeeded(request)
                } else {
                    Extract::Failed(msg)
                }
            }
        }
    };
}
for_all_payloads!(extractor);

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;

    #[test]
    fn reserialize_bisq_message() {
        let bisq = StoragePayload::decode(BISQ_HEX).unwrap();
        if let Some(storage_payload::Message::OfferPayload(_)) = bisq.message {
            assert!(true)
        } else {
            assert!(false)
        }
        let mut serialized = Vec::with_capacity(bisq.encoded_len());
        bisq.encode(&mut serialized)
            .expect("Could not encode message");
        assert!(&serialized == &BISQ_HEX);
        assert!(StoragePayload::decode(serialized) == Ok(bisq));
    }

    const BISQ_HEX: &[u8] = &[
        0x3A, 0x9D, 0x0A, 0x0A, 0x2F, 0x41, 0x4B, 0x52, 0x55, 0x56, 0x43, 0x2D, 0x38, 0x63, 0x38,
        0x30, 0x35, 0x61, 0x34, 0x39, 0x2D, 0x63, 0x31, 0x61, 0x33, 0x2D, 0x34, 0x35, 0x62, 0x34,
        0x2D, 0x39, 0x61, 0x30, 0x64, 0x2D, 0x30, 0x64, 0x36, 0x62, 0x63, 0x32, 0x65, 0x34, 0x63,
        0x32, 0x35, 0x35, 0x2D, 0x31, 0x31, 0x37, 0x10, 0xEF, 0xBF, 0xA9, 0xFC, 0xDB, 0x2D, 0x1A,
        0x1B, 0x0A, 0x16, 0x69, 0x6F, 0x71, 0x77, 0x75, 0x6F, 0x62, 0x6C, 0x33, 0x33, 0x6D, 0x66,
        0x63, 0x36, 0x63, 0x6B, 0x2E, 0x6F, 0x6E, 0x69, 0x6F, 0x6E, 0x10, 0x8F, 0x4E, 0x22, 0xE7,
        0x05, 0x0A, 0xBB, 0x03, 0x30, 0x82, 0x01, 0xB7, 0x30, 0x82, 0x01, 0x2C, 0x06, 0x07, 0x2A,
        0x86, 0x48, 0xCE, 0x38, 0x04, 0x01, 0x30, 0x82, 0x01, 0x1F, 0x02, 0x81, 0x81, 0x00, 0xFD,
        0x7F, 0x53, 0x81, 0x1D, 0x75, 0x12, 0x29, 0x52, 0xDF, 0x4A, 0x9C, 0x2E, 0xEC, 0xE4, 0xE7,
        0xF6, 0x11, 0xB7, 0x52, 0x3C, 0xEF, 0x44, 0x00, 0xC3, 0x1E, 0x3F, 0x80, 0xB6, 0x51, 0x26,
        0x69, 0x45, 0x5D, 0x40, 0x22, 0x51, 0xFB, 0x59, 0x3D, 0x8D, 0x58, 0xFA, 0xBF, 0xC5, 0xF5,
        0xBA, 0x30, 0xF6, 0xCB, 0x9B, 0x55, 0x6C, 0xD7, 0x81, 0x3B, 0x80, 0x1D, 0x34, 0x6F, 0xF2,
        0x66, 0x60, 0xB7, 0x6B, 0x99, 0x50, 0xA5, 0xA4, 0x9F, 0x9F, 0xE8, 0x04, 0x7B, 0x10, 0x22,
        0xC2, 0x4F, 0xBB, 0xA9, 0xD7, 0xFE, 0xB7, 0xC6, 0x1B, 0xF8, 0x3B, 0x57, 0xE7, 0xC6, 0xA8,
        0xA6, 0x15, 0x0F, 0x04, 0xFB, 0x83, 0xF6, 0xD3, 0xC5, 0x1E, 0xC3, 0x02, 0x35, 0x54, 0x13,
        0x5A, 0x16, 0x91, 0x32, 0xF6, 0x75, 0xF3, 0xAE, 0x2B, 0x61, 0xD7, 0x2A, 0xEF, 0xF2, 0x22,
        0x03, 0x19, 0x9D, 0xD1, 0x48, 0x01, 0xC7, 0x02, 0x15, 0x00, 0x97, 0x60, 0x50, 0x8F, 0x15,
        0x23, 0x0B, 0xCC, 0xB2, 0x92, 0xB9, 0x82, 0xA2, 0xEB, 0x84, 0x0B, 0xF0, 0x58, 0x1C, 0xF5,
        0x02, 0x81, 0x81, 0x00, 0xF7, 0xE1, 0xA0, 0x85, 0xD6, 0x9B, 0x3D, 0xDE, 0xCB, 0xBC, 0xAB,
        0x5C, 0x36, 0xB8, 0x57, 0xB9, 0x79, 0x94, 0xAF, 0xBB, 0xFA, 0x3A, 0xEA, 0x82, 0xF9, 0x57,
        0x4C, 0x0B, 0x3D, 0x07, 0x82, 0x67, 0x51, 0x59, 0x57, 0x8E, 0xBA, 0xD4, 0x59, 0x4F, 0xE6,
        0x71, 0x07, 0x10, 0x81, 0x80, 0xB4, 0x49, 0x16, 0x71, 0x23, 0xE8, 0x4C, 0x28, 0x16, 0x13,
        0xB7, 0xCF, 0x09, 0x32, 0x8C, 0xC8, 0xA6, 0xE1, 0x3C, 0x16, 0x7A, 0x8B, 0x54, 0x7C, 0x8D,
        0x28, 0xE0, 0xA3, 0xAE, 0x1E, 0x2B, 0xB3, 0xA6, 0x75, 0x91, 0x6E, 0xA3, 0x7F, 0x0B, 0xFA,
        0x21, 0x35, 0x62, 0xF1, 0xFB, 0x62, 0x7A, 0x01, 0x24, 0x3B, 0xCC, 0xA4, 0xF1, 0xBE, 0xA8,
        0x51, 0x90, 0x89, 0xA8, 0x83, 0xDF, 0xE1, 0x5A, 0xE5, 0x9F, 0x06, 0x92, 0x8B, 0x66, 0x5E,
        0x80, 0x7B, 0x55, 0x25, 0x64, 0x01, 0x4C, 0x3B, 0xFE, 0xCF, 0x49, 0x2A, 0x03, 0x81, 0x84,
        0x00, 0x02, 0x81, 0x80, 0x3B, 0x90, 0xBA, 0xB3, 0xCE, 0x46, 0xFC, 0x5C, 0x5C, 0x71, 0x04,
        0xD7, 0xBF, 0x11, 0xC6, 0x57, 0x70, 0x4A, 0x54, 0x45, 0x8A, 0xD1, 0xBB, 0x43, 0x90, 0x6D,
        0x43, 0x20, 0x71, 0xBB, 0x0E, 0x98, 0xF6, 0xFA, 0xE2, 0x61, 0x09, 0x32, 0xA4, 0xC9, 0x14,
        0xCB, 0x80, 0xEA, 0xCC, 0xE5, 0xBB, 0x90, 0xA3, 0x95, 0x10, 0x13, 0x5C, 0x0A, 0xDB, 0xE2,
        0x0D, 0xD5, 0xF6, 0xB8, 0xF9, 0xBC, 0x22, 0x95, 0x3A, 0x89, 0x4C, 0xA5, 0x65, 0x86, 0xD1,
        0x92, 0xD2, 0x8B, 0x75, 0x14, 0xCD, 0xB8, 0xDA, 0xA2, 0x6E, 0xEC, 0x5B, 0xD2, 0xA8, 0xD6,
        0x77, 0xDE, 0x30, 0xD1, 0x2A, 0xAB, 0xEA, 0xF5, 0x44, 0x7D, 0x17, 0x2F, 0x50, 0x88, 0x0D,
        0xA0, 0xC1, 0xD5, 0x6E, 0xF6, 0x4D, 0x39, 0xD3, 0x0A, 0xD2, 0x71, 0xD1, 0xF2, 0xE1, 0x28,
        0xAF, 0x32, 0x32, 0xC3, 0x45, 0x62, 0x6D, 0x7B, 0x97, 0xE1, 0x34, 0xB0, 0x12, 0xA6, 0x02,
        0x30, 0x82, 0x01, 0x22, 0x30, 0x0D, 0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01,
        0x01, 0x01, 0x05, 0x00, 0x03, 0x82, 0x01, 0x0F, 0x00, 0x30, 0x82, 0x01, 0x0A, 0x02, 0x82,
        0x01, 0x01, 0x00, 0xA9, 0x8A, 0xE6, 0x0F, 0x6F, 0x97, 0x14, 0x77, 0x05, 0x93, 0xED, 0xA9,
        0x97, 0xB9, 0xC2, 0x2B, 0x5D, 0xA6, 0x7D, 0x31, 0xC3, 0xB7, 0x8F, 0x3B, 0xA4, 0xC3, 0xAF,
        0x86, 0xBF, 0x31, 0xC1, 0x3B, 0xB1, 0xD7, 0x47, 0x7E, 0x5E, 0xC5, 0x3A, 0xDD, 0xA2, 0x6B,
        0xD5, 0xBC, 0x1B, 0x07, 0x37, 0xEA, 0xA3, 0xEE, 0x7D, 0x1C, 0xAA, 0x4F, 0xA8, 0x1D, 0x19,
        0xD3, 0x1F, 0x05, 0xA5, 0x39, 0x6E, 0xA1, 0x46, 0xB7, 0xAA, 0xB7, 0xAF, 0xB5, 0x36, 0x62,
        0x32, 0x0B, 0x88, 0xC7, 0xBC, 0x40, 0xF3, 0x30, 0xA8, 0x13, 0xDF, 0xDE, 0xB9, 0x4E, 0x88,
        0x8C, 0xAF, 0xF6, 0xDD, 0x7E, 0x9A, 0x4A, 0xE4, 0xDD, 0xD4, 0xF4, 0x7A, 0x81, 0xC9, 0x46,
        0xC8, 0x53, 0xC9, 0x92, 0x54, 0x17, 0x90, 0xF0, 0x95, 0xDC, 0x24, 0x86, 0xEC, 0x1C, 0xE7,
        0xB8, 0x6F, 0xF9, 0x7E, 0x06, 0xD2, 0x13, 0x63, 0xE0, 0xCE, 0xC7, 0xD3, 0xE0, 0x85, 0xE4,
        0x12, 0x67, 0x62, 0x08, 0x7E, 0xA9, 0xBB, 0xA8, 0x75, 0x00, 0xEF, 0xF3, 0xC8, 0x34, 0xB9,
        0x4C, 0xF6, 0xCB, 0x6F, 0xC8, 0x6C, 0x64, 0x4D, 0x83, 0x2E, 0xA9, 0xBF, 0xCB, 0xD4, 0x88,
        0x31, 0x54, 0x9C, 0x34, 0xB8, 0xC1, 0xC4, 0x8E, 0xE9, 0xBB, 0x2B, 0x59, 0xF0, 0x89, 0x2D,
        0x1A, 0xF7, 0x8C, 0xB2, 0x2A, 0x20, 0xC5, 0x25, 0x7F, 0x81, 0xDA, 0x7B, 0xEA, 0xF1, 0xAF,
        0x7B, 0x1F, 0x56, 0x6E, 0xCA, 0x8A, 0x52, 0xFF, 0x8C, 0x36, 0x5A, 0xA6, 0x6D, 0xAC, 0x09,
        0xE6, 0x59, 0x26, 0x57, 0x36, 0xB7, 0xBD, 0xB8, 0x33, 0xCB, 0x37, 0x13, 0x47, 0x9A, 0xBB,
        0x42, 0x12, 0x46, 0x21, 0xD1, 0xC2, 0x93, 0x61, 0xC4, 0x69, 0xC2, 0x5A, 0x15, 0x90, 0x42,
        0xC0, 0x79, 0x5A, 0x76, 0x2B, 0x63, 0x75, 0x91, 0x73, 0x25, 0xE6, 0x93, 0xF6, 0xF2, 0xF7,
        0x82, 0xE7, 0x92, 0x33, 0x02, 0x03, 0x01, 0x00, 0x01, 0x28, 0x02, 0x39, 0x0A, 0xD7, 0xA3,
        0x70, 0x3D, 0x0A, 0xC7, 0x3F, 0x40, 0x01, 0x48, 0x80, 0x92, 0xF4, 0x01, 0x50, 0xC0, 0x84,
        0x3D, 0x5A, 0x03, 0x42, 0x54, 0x43, 0x62, 0x03, 0x55, 0x53, 0x44, 0x6A, 0x1B, 0x0A, 0x16,
        0x65, 0x78, 0x69, 0x68, 0x6D, 0x66, 0x7A, 0x61, 0x35, 0x33, 0x34, 0x33, 0x65, 0x68, 0x37,
        0x71, 0x2E, 0x6F, 0x6E, 0x69, 0x6F, 0x6E, 0x10, 0x8F, 0x4E, 0x6A, 0x1B, 0x0A, 0x16, 0x6C,
        0x78, 0x74, 0x61, 0x6B, 0x62, 0x32, 0x69, 0x74, 0x61, 0x76, 0x7A, 0x76, 0x35, 0x77, 0x37,
        0x2E, 0x6F, 0x6E, 0x69, 0x6F, 0x6E, 0x10, 0x8F, 0x4E, 0x72, 0x1B, 0x0A, 0x16, 0x65, 0x78,
        0x69, 0x68, 0x6D, 0x66, 0x7A, 0x61, 0x35, 0x33, 0x34, 0x33, 0x65, 0x68, 0x37, 0x71, 0x2E,
        0x6F, 0x6E, 0x69, 0x6F, 0x6E, 0x10, 0x8F, 0x4E, 0x72, 0x1B, 0x0A, 0x16, 0x6C, 0x78, 0x74,
        0x61, 0x6B, 0x62, 0x32, 0x69, 0x74, 0x61, 0x76, 0x7A, 0x76, 0x35, 0x77, 0x37, 0x2E, 0x6F,
        0x6E, 0x69, 0x6F, 0x6E, 0x10, 0x8F, 0x4E, 0x7A, 0x03, 0x46, 0x32, 0x46, 0x82, 0x01, 0x24,
        0x64, 0x33, 0x37, 0x30, 0x36, 0x61, 0x65, 0x61, 0x2D, 0x37, 0x32, 0x63, 0x34, 0x2D, 0x34,
        0x63, 0x31, 0x31, 0x2D, 0x38, 0x63, 0x30, 0x39, 0x2D, 0x64, 0x30, 0x36, 0x61, 0x34, 0x66,
        0x30, 0x66, 0x32, 0x64, 0x66, 0x61, 0x8A, 0x01, 0x40, 0x38, 0x30, 0x31, 0x66, 0x65, 0x33,
        0x63, 0x66, 0x33, 0x39, 0x34, 0x36, 0x64, 0x39, 0x30, 0x64, 0x32, 0x33, 0x66, 0x37, 0x35,
        0x62, 0x38, 0x66, 0x61, 0x31, 0x33, 0x37, 0x35, 0x39, 0x62, 0x63, 0x33, 0x64, 0x35, 0x61,
        0x35, 0x66, 0x38, 0x61, 0x64, 0x35, 0x33, 0x37, 0x31, 0x38, 0x39, 0x33, 0x63, 0x65, 0x66,
        0x37, 0x36, 0x39, 0x32, 0x66, 0x32, 0x63, 0x63, 0x36, 0x30, 0x34, 0x36, 0x65, 0x92, 0x01,
        0x02, 0x4C, 0x41, 0x9A, 0x01, 0x02, 0x4C, 0x41, 0xB2, 0x01, 0x05, 0x31, 0x2E, 0x31, 0x2E,
        0x37, 0xB8, 0x01, 0x82, 0xC8, 0x24, 0xC0, 0x01, 0x8A, 0x37, 0xC8, 0x01, 0x34, 0xD8, 0x01,
        0x80, 0xB5, 0x18, 0xE0, 0x01, 0xA0, 0xC2, 0x1E, 0xE8, 0x01, 0xC0, 0xF0, 0xF5, 0x0B, 0xF0,
        0x01, 0x80, 0xE0, 0xE5, 0xA4, 0x01, 0xAA, 0x02, 0x2E, 0x0A, 0x0C, 0x63, 0x61, 0x70, 0x61,
        0x62, 0x69, 0x6C, 0x69, 0x74, 0x69, 0x65, 0x73, 0x12, 0x1E, 0x30, 0x2C, 0x20, 0x31, 0x2C,
        0x20, 0x32, 0x2C, 0x20, 0x35, 0x2C, 0x20, 0x36, 0x2C, 0x20, 0x37, 0x2C, 0x20, 0x38, 0x2C,
        0x20, 0x39, 0x2C, 0x20, 0x31, 0x30, 0x2C, 0x20, 0x31, 0x32, 0xAA, 0x02, 0x41, 0x0A, 0x15,
        0x61, 0x63, 0x63, 0x6F, 0x75, 0x6E, 0x74, 0x41, 0x67, 0x65, 0x57, 0x69, 0x74, 0x6E, 0x65,
        0x73, 0x73, 0x48, 0x61, 0x73, 0x68, 0x12, 0x28, 0x63, 0x62, 0x35, 0x64, 0x63, 0x61, 0x64,
        0x37, 0x66, 0x64, 0x64, 0x62, 0x30, 0x32, 0x63, 0x66, 0x37, 0x37, 0x36, 0x36, 0x35, 0x32,
        0x38, 0x30, 0x61, 0x33, 0x64, 0x39, 0x63, 0x37, 0x35, 0x36, 0x66, 0x38, 0x33, 0x63, 0x36,
        0x33, 0x34, 0x63, 0xAA, 0x02, 0x10, 0x0A, 0x0C, 0x66, 0x32, 0x66, 0x45, 0x78, 0x74, 0x72,
        0x61, 0x49, 0x6E, 0x66, 0x6F, 0x12, 0x00, 0xAA, 0x02, 0x18, 0x0A, 0x07, 0x66, 0x32, 0x66,
        0x43, 0x69, 0x74, 0x79, 0x12, 0x0D, 0x4C, 0x55, 0x41, 0x4E, 0x47, 0x20, 0x50, 0x52, 0x41,
        0x42, 0x41, 0x4E, 0x47, 0xB0, 0x02, 0x01,
    ];
}
