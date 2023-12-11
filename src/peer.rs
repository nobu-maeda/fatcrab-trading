use std::any::Any;

use crusty_n3xb::{common::types::SerdeGenericTrait, peer_msg::PeerEnvelope};
use serde::{Deserialize, Serialize};

pub struct FatCrabPeerEnvelope {
    pub message: FatCrabPeerMessage,
    pub(crate) _envelope: PeerEnvelope,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FatCrabPeerMessage {
    pub receive_address: String,
    pub txid: String,
}

#[typetag::serde(name = "fatcrab_peer_message_specifics")]
impl SerdeGenericTrait for FatCrabPeerMessage {
    fn any_ref(&self) -> &dyn Any {
        self
    }
}
