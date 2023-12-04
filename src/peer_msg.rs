use crusty_n3xb::peer_msg::PeerEnvelope;

pub struct FatCrabPeerEnvelope {
    pub peer_msg: FatCrabPeerMsg,
    pub(crate) envelope: PeerEnvelope,
}

pub struct FatCrabPeerMsg {}
