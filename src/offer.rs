use core::panic;
use std::any::Any;

use crusty_n3xb::{
    common::types::{BitcoinSettlementMethod, ObligationKind, SerdeGenericTrait},
    offer::{Obligation, Offer, OfferBuilder},
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

pub enum FatCrabOffer {
    Buy { bitcoin_addr: String },
    Sell { fatcrab_acct_id: Uuid },
}

impl FatCrabOffer {
    pub(crate) fn into(&self, order: FatCrabOrder) -> Offer {
        let mut builder = OfferBuilder::new();

        match self {
            FatCrabOffer::Buy { bitcoin_addr } => match order {
                FatCrabOrder::Sell { .. } => {
                    panic!("FatCrab Buy Offer & FatCrab Sell Order mismatches");
                }
                FatCrabOrder::Buy {
                    amount,          // in FC
                    price,           // in sats / FC
                    fatcrab_acct_id, // Maker to rx FC
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
            FatCrabOffer::Sell { fatcrab_acct_id } => match order {
                FatCrabOrder::Buy { .. } => {
                    panic!("FatCrab Sell Offer & FatCrab Buy Order mismatches");
                }
                FatCrabOrder::Sell {
                    amount,       // in FC
                    price,        // in sats / FC
                    bitcoin_addr, // Maker to rx Bitcoin
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
