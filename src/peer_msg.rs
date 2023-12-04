use std::any::Any;

use crusty_n3xb::{common::types::SerdeGenericTrait, peer_msg::PeerEnvelope};
use serde::{Deserialize, Serialize};

pub struct FatCrabPeerEnvelope {
    pub peer_msg: FatCrabPeerMsg,
    pub(crate) envelope: PeerEnvelope,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FatCrabPeerMsg {}

#[typetag::serde(name = "fatcrab_peer_message_specifics")]
impl SerdeGenericTrait for FatCrabPeerMsg {
    fn any_ref(&self) -> &dyn Any {
        self
    }
}
