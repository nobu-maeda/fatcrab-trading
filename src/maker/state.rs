use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum FatCrabMakerState {
    New,
    WaitingForOffers,
    ReceivedOffer,
    AcceptedOffer,
    InboundBtcNotified,
    InboundFcNotified,
    NotifiedOutbound,
    TradeCompleted,
    TradeCancelled,
}
