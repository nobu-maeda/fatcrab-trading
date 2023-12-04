use std::{any::Any, collections::HashSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crusty_n3xb::{
    common::types::{BitcoinSettlementMethod, ObligationKind, SerdeGenericTrait},
    order::{
        MakerObligation, MakerObligationContent, Order, OrderBuilder, OrderEnvelope,
        TakerObligation, TakerObligationContent, TradeDetails, TradeDetailsContent, TradeParameter,
    },
};

use crate::{common::FATCRAB_OBLIGATION_CUSTOM_KIND_STRING, error::FatCrabError};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FatCrabMakeOrderSpecifics {
    receive_address: String,
}

#[typetag::serde(name = "fatcrab_make_order_specifics")]
impl SerdeGenericTrait for FatCrabMakeOrderSpecifics {
    fn any_ref(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone, PartialEq)]
pub enum FatCrabOrderType {
    Buy,
    Sell,
}

#[derive(Debug, Clone)]
pub struct FatCrabOrderEnvelope {
    pub order: FatCrabOrder,
    pub(crate) envelope: OrderEnvelope,
}

#[derive(Debug, Clone)]
pub enum FatCrabOrder {
    Buy {
        trade_uuid: Uuid,
        amount: f64,           // in FC
        price: f64,            // in sats / FC
        fatcrab_acct_id: Uuid, // to receive FCs
    },
    Sell {
        trade_uuid: Uuid,
        amount: f64,          // in FC
        price: f64,           // in sats / FC
        bitcoin_addr: String, // to receive BTC
    },
}

// Quick Reference for FatCrab Order to n3xB Order Maker/Taker Obligation
// FatCrab Buy Orders - Amount in FC, Price in BTC(sats)/FC
// n3xB Orders - Maker Obligation BTC, Amount in sats, receives FCs. Taker Obligation FC, Limit rate in #FC/#Sats = 1/Price
// FatCrab Sell Orders - Amount in FC, Price in BTC(sats)/FC
// n3xB Orders - Maker Obligation in FC, Amount in FC, receives sats. Taker Obligation BTC, Limit rate in #Sats/#FC = Price

impl FatCrabOrder {
    pub fn trade_uuid(&self) -> &Uuid {
        match self {
            Self::Buy { trade_uuid, .. } => trade_uuid,
            Self::Sell { trade_uuid, .. } => trade_uuid,
        }
    }

    pub(crate) fn from_n3xb_order(order: Order) -> Result<Self, FatCrabError> {
        let mut amount: Option<f64> = None;
        let mut price: Option<f64> = None;
        let mut fatcrab_order_kind: Option<FatCrabOrderType> = None;
        let mut intended_order_kind: Option<FatCrabOrderType> = None;

        // Look for either a Bitcoin Onchain or a Fatcrab Obligation Kind
        for kind in order.maker_obligation.kinds {
            match kind {
                ObligationKind::Custom(string) => {
                    if string == FATCRAB_OBLIGATION_CUSTOM_KIND_STRING {
                        intended_order_kind = Some(FatCrabOrderType::Sell);
                        amount = Some(order.maker_obligation.content.amount);
                        price = Some(order.taker_obligation.content.limit_rate.unwrap());
                    }
                }
                ObligationKind::Bitcoin(settlement) => {
                    if let Some(settlement) = settlement {
                        if settlement == BitcoinSettlementMethod::Onchain {
                            intended_order_kind = Some(FatCrabOrderType::Buy);
                            let bitcoin_sat_amount = order.maker_obligation.content.amount;
                            let limit_rate = order.taker_obligation.content.limit_rate.unwrap();
                            price = Some(1.0 / limit_rate);
                            amount = Some(bitcoin_sat_amount as f64 * limit_rate);
                        }
                    }
                }
                _ => continue,
            }

            if let Some(intended_kind) = intended_order_kind.clone() {
                if let Some(kind) = fatcrab_order_kind.clone() {
                    if kind != intended_kind {
                        return Err(FatCrabError::Simple {
                            description:
                                "FatCrabOrder::from() - Mismatch kinds in Order Maker Obligation"
                                    .to_string(),
                        });
                    }
                } else {
                    fatcrab_order_kind = Some(intended_kind);
                }
            }
        }

        // Look for either a Bitcoin Onchain or a Fatcrab Obligation Kind
        for kind in order.taker_obligation.kinds {
            match kind {
                ObligationKind::Custom(string) => {
                    if string == FATCRAB_OBLIGATION_CUSTOM_KIND_STRING {
                        intended_order_kind = Some(FatCrabOrderType::Buy);
                    }
                }
                ObligationKind::Bitcoin(settlement) => {
                    if let Some(settlement) = settlement {
                        if settlement == BitcoinSettlementMethod::Onchain {
                            intended_order_kind = Some(FatCrabOrderType::Sell);
                        }
                    }
                }
                _ => continue,
            }

            if let Some(intended_kind) = intended_order_kind.clone() {
                if let Some(kind) = fatcrab_order_kind.clone() {
                    if kind != intended_kind {
                        return Err(FatCrabError::Simple {
                            description:
                                "FatCrabOrder::from() - Mismatch kinds in Order Taker Obligation"
                                    .to_string(),
                        });
                    }
                } else {
                    panic!("Internal Inconsistency - fatcrab_order_kind should already have been set by maker");
                }
            }
        }

        if let (Some(fatcrab_order_kind), Some(amount), Some(price)) =
            (fatcrab_order_kind, amount, price)
        {
            let fatcrab_specifics = order
                .trade_engine_specifics
                .downcast_ref::<FatCrabMakeOrderSpecifics>()
                .unwrap();

            match fatcrab_order_kind {
                FatCrabOrderType::Buy => {
                    return Ok(Self::Buy {
                        trade_uuid: order.trade_uuid,
                        amount,
                        price,
                        fatcrab_acct_id: Uuid::parse_str(
                            fatcrab_specifics.receive_address.as_str(),
                        )
                        .unwrap(),
                    })
                }
                FatCrabOrderType::Sell => {
                    return Ok(Self::Sell {
                        trade_uuid: order.trade_uuid,
                        amount,
                        price,
                        bitcoin_addr: fatcrab_specifics.receive_address.clone(),
                    })
                }
            }
        } else {
            Err(FatCrabError::Simple {
                description: "FatCrabOrder::from() - Could not determine order type".to_string(),
            })
        }
    }
}

impl Into<Order> for FatCrabOrder {
    fn into(self) -> Order {
        let mut builder = OrderBuilder::new();

        match self {
            Self::Buy {
                trade_uuid,
                amount,
                price,
                fatcrab_acct_id,
            } => {
                builder.trade_uuid(trade_uuid);

                let maker_obligation_kind =
                    ObligationKind::Bitcoin(Some(BitcoinSettlementMethod::Onchain));
                let maker_obligation_kinds =
                    HashSet::from_iter(vec![maker_obligation_kind].into_iter());
                let maker_obligation_content = MakerObligationContent {
                    amount,
                    amount_min: None,
                };

                let maker_obligation = MakerObligation {
                    kinds: maker_obligation_kinds,
                    content: maker_obligation_content,
                };

                builder.maker_obligation(maker_obligation);

                let taker_obligation_kind = ObligationKind::Custom("FatCrab".to_string());
                let taker_obligation_kinds =
                    HashSet::from_iter(vec![taker_obligation_kind].into_iter());
                let taker_obligation_content = TakerObligationContent {
                    limit_rate: Some(1.0 / price),
                    market_offset_pct: None,
                    market_oracles: None,
                };

                let taker_obligation = TakerObligation {
                    kinds: taker_obligation_kinds,
                    content: taker_obligation_content,
                };

                builder.taker_obligation(taker_obligation);

                let trade_engine_specifics = FatCrabMakeOrderSpecifics {
                    receive_address: fatcrab_acct_id.to_string(),
                };
                builder.trade_engine_specifics(Box::new(trade_engine_specifics));
            }
            Self::Sell {
                trade_uuid,
                amount,
                price,
                bitcoin_addr,
            } => {
                builder.trade_uuid(trade_uuid);

                let maker_obligation_kind = ObligationKind::Custom("FatCrab".to_string());
                let maker_obligation_kinds =
                    HashSet::from_iter(vec![maker_obligation_kind].into_iter());
                let maker_obligation_content = MakerObligationContent {
                    amount,
                    amount_min: None,
                };

                let maker_obligation = MakerObligation {
                    kinds: maker_obligation_kinds,
                    content: maker_obligation_content,
                };

                builder.maker_obligation(maker_obligation);

                let taker_obligation_kind =
                    ObligationKind::Bitcoin(Some(BitcoinSettlementMethod::Onchain));
                let taker_obligation_kinds =
                    HashSet::from_iter(vec![taker_obligation_kind].into_iter());
                let taker_obligation_content = TakerObligationContent {
                    limit_rate: Some(price),
                    market_offset_pct: None,
                    market_oracles: None,
                };

                let taker_obligation = TakerObligation {
                    kinds: taker_obligation_kinds,
                    content: taker_obligation_content,
                };

                builder.taker_obligation(taker_obligation);

                let trade_engine_specifics = FatCrabMakeOrderSpecifics {
                    receive_address: bitcoin_addr,
                };
                builder.trade_engine_specifics(Box::new(trade_engine_specifics));
            }
        }

        let trade_parameters = HashSet::<TradeParameter>::new();
        let trade_details_content = TradeDetailsContent {
            maker_bond_pct: None,
            taker_bond_pct: None,
            trade_timeout: None,
        };

        let trade_details = TradeDetails {
            parameters: trade_parameters,
            content: trade_details_content,
        };

        builder.trade_details(trade_details);
        builder.build().unwrap()
    }
}
