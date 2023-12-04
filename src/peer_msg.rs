use std::any::Any;

use crusty_n3xb::{common::types::SerdeGenericTrait, peer_msg::PeerEnvelope};
use serde::{Deserialize, Serialize};

pub struct FatCrabPeerEnvelope {
    pub peer_msg: FatCrabPeerMessage,
    pub(crate) envelope: PeerEnvelope,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FatCrabPeerMessage {
    FatCrabRemitted {
        fatcrab_txid: String,
        bitcoin_address: String,
    },
    BitcoinRemitted {
        bitcoin_txid: String,
    },
}

#[typetag::serde(name = "fatcrab_peer_message_specifics")]
impl SerdeGenericTrait for FatCrabPeerMessage {
    fn any_ref(&self) -> &dyn Any {
        self
    }
}
