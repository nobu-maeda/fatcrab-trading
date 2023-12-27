use std::any::Any;

use crusty_n3xb::{common::types::SerdeGenericTrait, peer_msg::PeerEnvelope};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct FatCrabPeerEnvelope {
    pub message: FatCrabPeerMessage,
    pub(crate) _envelope: PeerEnvelope,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FatCrabPeerMessage {
    pub receive_address: String,
    pub txid: String,
}

// Workaround to make FFI happy...
// True reason for violation is a Box<dyn SerdeGenericTrait> deep inside PeerEnvelope
unsafe impl Sync for FatCrabPeerEnvelope {}
unsafe impl Send for FatCrabPeerEnvelope {}

#[typetag::serde(name = "fatcrab_peer_message_specifics")]
impl SerdeGenericTrait for FatCrabPeerMessage {
    fn any_ref(&self) -> &dyn Any {
        self
    }
}
