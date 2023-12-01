use std::{any::Any, collections::HashSet};

use crusty_n3xb::{
    common::types::{BitcoinSettlementMethod, ObligationKind, SerdeGenericTrait},
    order::{
        MakerObligation, MakerObligationContent, Order, OrderBuilder, TakerObligation,
        TakerObligationContent, TradeDetails, TradeDetailsContent, TradeParameter,
    },
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

#[derive(Clone)]
pub enum FatCrabOrder {
    Buy {
        amount: u64,
        price: f64,
        fatcrab_acct_id: Uuid,
    },
    Sell {
        amount: u64,
        price: f64,
        bitcoin_addr: String,
    },
}

impl Into<Order> for FatCrabOrder {
    fn into(self) -> Order {
        let mut builder = OrderBuilder::new();

        match self {
            FatCrabOrder::Buy {
                amount,
                price,
                fatcrab_acct_id,
            } => {
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
            FatCrabOrder::Sell {
                amount,
                price,
                bitcoin_addr,
            } => {
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
