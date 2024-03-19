use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FatCrabTakerState {
    New,
    SubmittedOffer,
    OfferAccepted,
    OfferRejected,
    NotifiedOutbound,
    InboundBtcNotified,
    InboundFcNotified,
    TradeCompleted,
}
