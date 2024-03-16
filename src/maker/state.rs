use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FatCrabMakerState {
    New,
    WaitingForOffers,
    ReceivedOffer,
    AcceptedOffer,
    InboundBtcNotified,
    InboundFcNotified,
    NotifiedOutbound,
    TradeCompleted,
}
