use std::any::Any;

use crusty_n3xb::{common::types::SerdeGenericTrait, peer_msg::PeerEnvelope};
use serde::{Deserialize, Serialize};

pub struct FatCrabPeerEnvelope {
    pub message: FatCrabPeerMessage,
    pub(crate) envelope: PeerEnvelope,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FatCrabPeerMessage {
    receive_address: Option<String>,
    txid: Option<String>,
}

#[typetag::serde(name = "fatcrab_peer_message_specifics")]
impl SerdeGenericTrait for FatCrabPeerMessage {
    fn any_ref(&self) -> &dyn Any {
        self
    }
}
