use core::panic;
use std::any::Any;

use crusty_n3xb::{
    common::types::{BitcoinSettlementMethod, ObligationKind, SerdeGenericTrait},
    offer::{Obligation, Offer, OfferBuilder, OfferEnvelope},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    common::FATCRAB_OBLIGATION_CUSTOM_KIND_STRING,
    error::FatCrabError,
    order::{FatCrabOrder, FatCrabOrderType},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FatCrabTakeOrderSpecifics {
    receive_address: String,
}

#[typetag::serde(name = "fatcrab_take_order_specifics")]
impl SerdeGenericTrait for FatCrabTakeOrderSpecifics {
    fn any_ref(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug, Clone)]
pub struct FatCrabOfferEnvelope {
    pub offer: Offer,
    pub(crate) envelope: OfferEnvelope,
}

#[derive(Debug, Clone)]
pub enum FatCrabOffer {
    Buy { bitcoin_addr: String },
    Sell { fatcrab_acct_id: Uuid },
}

impl FatCrabOffer {
    pub(crate) fn from_n3xb_offer(offer: Offer) -> Result<Self, FatCrabError> {
        let order_type = match &offer.maker_obligation.kind {
            ObligationKind::Bitcoin { .. } => FatCrabOrderType::Buy,
            ObligationKind::Custom(kind) => {
                if kind == FATCRAB_OBLIGATION_CUSTOM_KIND_STRING {
                    FatCrabOrderType::Sell
                } else {
                    return Err(FatCrabError::Simple {
                        description: format!(
                            "Offer Maker Obligation Kind Custom for {} not expected",
                            kind
                        ),
                    });
                }
            }
            ObligationKind::Fiat(currency, _method) => {
                return Err(FatCrabError::Simple {
                    description: format!(
                        "Offer Maker Obligation Kind Fiat for {} not expected",
                        currency
                    ),
                });
            }
        };

        let internal_inconsistent = match &offer.taker_obligation.kind {
            ObligationKind::Bitcoin { .. } => order_type != FatCrabOrderType::Sell,
            ObligationKind::Custom(kind) => {
                if kind == FATCRAB_OBLIGATION_CUSTOM_KIND_STRING {
                    order_type != FatCrabOrderType::Buy
                } else {
                    return Err(FatCrabError::Simple {
                        description: format!(
                            "Offer Taker Obligation Kind Custom for {} not expected",
                            kind
                        ),
                    });
                }
            }
            ObligationKind::Fiat(currency, _method) => {
                return Err(FatCrabError::Simple {
                    description: format!(
                        "Offer Taker Obligation Kind Fiat for {} not expected",
                        currency
                    ),
                });
            }
        };

        if internal_inconsistent {
            return Err(FatCrabError::Simple {
                description: format!(
                    "Offer Obligation Kinds internally inconsistent - Maker: {:?}, Taker: {:?}",
                    offer.maker_obligation.kind, offer.taker_obligation.kind
                ),
            });
        }

        let fatcrab_offer_specifics = offer
            .trade_engine_specifics
            .downcast_ref::<FatCrabTakeOrderSpecifics>()
            .ok_or_else(|| FatCrabError::Simple {
                description: "Offer Trade Engine Specifics not expected".to_string(),
            })?;

        let receive_address = fatcrab_offer_specifics.receive_address.clone();

        let offer = match order_type {
            FatCrabOrderType::Buy => Self::Buy {
                bitcoin_addr: receive_address,
            },
            FatCrabOrderType::Sell => Self::Sell {
                fatcrab_acct_id: Uuid::parse_str(&receive_address).map_err(|e| {
                    FatCrabError::Simple {
                        description: format!(
                            "Offer Trade Engine Specifics Receive Address not a valid UUID: {}",
                            e
                        ),
                    }
                })?,
            },
        };

        Ok(offer)
    }

    pub(crate) fn into_n3xb_offer(&self, order: FatCrabOrder) -> Offer {
        let mut builder = OfferBuilder::new();

        match self {
            Self::Buy { bitcoin_addr } => match order {
                FatCrabOrder::Sell { .. } => {
                    panic!("FatCrab Buy Offer & FatCrab Sell Order mismatches");
                }
                FatCrabOrder::Buy {
                    trade_uuid: _,
                    amount,             // in FC
                    price,              // in sats / FC
                    fatcrab_acct_id: _, // Maker to rx FC
                } => {
                    let maker_obligation = Obligation {
                        kind: ObligationKind::Bitcoin(Some(BitcoinSettlementMethod::Onchain)),
                        amount: (amount * price).round(), // Sat amount in fraction is not allowed
                        bond_amount: None,
                    };
                    let taker_obligation = Obligation {
                        kind: ObligationKind::Custom(
                            FATCRAB_OBLIGATION_CUSTOM_KIND_STRING.to_string(),
                        ),
                        amount: amount,
                        bond_amount: None,
                    };
                    let specifics = FatCrabTakeOrderSpecifics {
                        receive_address: bitcoin_addr.clone(),
                    };
                    builder.maker_obligation(maker_obligation);
                    builder.taker_obligation(taker_obligation);
                    builder.trade_engine_specifics(Box::new(specifics));
                }
            },
            Self::Sell { fatcrab_acct_id } => match order {
                FatCrabOrder::Buy { .. } => {
                    panic!("FatCrab Sell Offer & FatCrab Buy Order mismatches");
                }
                FatCrabOrder::Sell {
                    trade_uuid: _,
                    amount,          // in FC
                    price,           // in sats / FC
                    bitcoin_addr: _, // Maker to rx Bitcoin
                } => {
                    let maker_obligation = Obligation {
                        kind: ObligationKind::Custom(
                            FATCRAB_OBLIGATION_CUSTOM_KIND_STRING.to_string(),
                        ),
                        amount,
                        bond_amount: None,
                    };
                    let taker_obligation = Obligation {
                        kind: ObligationKind::Bitcoin(Some(BitcoinSettlementMethod::Onchain)),
                        amount: (amount * price).round(), // Sat amount in fraction is not allowed
                        bond_amount: None,
                    };
                    let specifics = FatCrabTakeOrderSpecifics {
                        receive_address: fatcrab_acct_id.to_string(),
                    };
                    builder.maker_obligation(maker_obligation);
                    builder.taker_obligation(taker_obligation);
                    builder.trade_engine_specifics(Box::new(specifics));
                }
            },
        }
        builder.build().unwrap()
    }
}
