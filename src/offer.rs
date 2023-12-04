use std::any::Any;

use crusty_n3xb::{
    common::types::{BitcoinSettlementMethod, ObligationKind, SerdeGenericTrait},
    offer::{Obligation, Offer, OfferBuilder, OfferEnvelope},
};
use serde::{Deserialize, Serialize};

use crate::{
    common::FATCRAB_OBLIGATION_CUSTOM_KIND_STRING,
    error::FatCrabError,
    order::{FatCrabOrder, FatCrabOrderType},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FatCrabTakeOrderSpecifics {}

#[typetag::serde(name = "fatcrab_take_order_specifics")]
impl SerdeGenericTrait for FatCrabTakeOrderSpecifics {
    fn any_ref(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug, Clone)]
pub struct FatCrabOfferEnvelope {
    pub offer: FatCrabOffer,
    pub(crate) envelope: OfferEnvelope,
}

#[derive(Debug, Clone)]
pub enum FatCrabOffer {
    Buy,
    Sell,
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

        match order_type {
            FatCrabOrderType::Buy => Ok(Self::Buy),
            FatCrabOrderType::Sell => Ok(Self::Sell),
        }
    }

    pub(crate) fn into_n3xb_offer(&self, order: FatCrabOrder) -> Offer {
        let mut builder = OfferBuilder::new();

        match self {
            FatCrabOffer::Buy => {
                assert_eq!(order.order_type, FatCrabOrderType::Buy);
                let maker_obligation = Obligation {
                    kind: ObligationKind::Bitcoin(Some(BitcoinSettlementMethod::Onchain)),
                    amount: (order.amount * order.price).round(), // Sat amount in fraction is not allowed
                    bond_amount: None,
                };
                let taker_obligation = Obligation {
                    kind: ObligationKind::Custom(FATCRAB_OBLIGATION_CUSTOM_KIND_STRING.to_string()),
                    amount: order.amount,
                    bond_amount: None,
                };
                let specifics = FatCrabTakeOrderSpecifics {};
                builder.maker_obligation(maker_obligation);
                builder.taker_obligation(taker_obligation);
                builder.trade_engine_specifics(Box::new(specifics));
            }

            FatCrabOffer::Sell => {
                assert_eq!(order.order_type, FatCrabOrderType::Sell);
                let maker_obligation = Obligation {
                    kind: ObligationKind::Custom(FATCRAB_OBLIGATION_CUSTOM_KIND_STRING.to_string()),
                    amount: order.amount,
                    bond_amount: None,
                };
                let taker_obligation = Obligation {
                    kind: ObligationKind::Bitcoin(Some(BitcoinSettlementMethod::Onchain)),
                    amount: (order.amount * order.price).round(), // Sat amount in fraction is not allowed
                    bond_amount: None,
                };
                let specifics = FatCrabTakeOrderSpecifics {};
                builder.maker_obligation(maker_obligation);
                builder.taker_obligation(taker_obligation);
                builder.trade_engine_specifics(Box::new(specifics));
            }
        }
        builder.build().unwrap()
    }
}
