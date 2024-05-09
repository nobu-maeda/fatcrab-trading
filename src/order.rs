use std::{any::Any, collections::HashSet};

use bitcoin::Network;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crusty_n3xb::{
    common::types::{BitcoinNetwork, BitcoinSettlementMethod, ObligationKind, SerdeGenericTrait},
    order::{
        MakerObligation, MakerObligationContent, Order, OrderBuilder, OrderEnvelope,
        TakerObligation, TakerObligationContent, TradeDetails, TradeDetailsContent, TradeParameter,
    },
};

use crate::{common::FATCRAB_OBLIGATION_CUSTOM_KIND_STRING, error::FatCrabError};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FatCrabMakeOrderSpecifics {}

#[typetag::serde(name = "fatcrab_make_order_specifics")]
impl SerdeGenericTrait for FatCrabMakeOrderSpecifics {
    fn any_ref(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FatCrabOrderType {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FatCrabOrderEnvelope {
    pub order: FatCrabOrder,
    pub pubkey: String,
    pub(crate) envelope: OrderEnvelope,
}

// Workaround to make FFI happy...
// True reason for violation is a Box<dyn SerdeGenericTrait> deep inside OrderEnvelope
unsafe impl Sync for FatCrabOrderEnvelope {}
unsafe impl Send for FatCrabOrderEnvelope {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FatCrabOrder {
    pub order_type: FatCrabOrderType,
    pub trade_uuid: Uuid,
    pub amount: f64, // in FC
    pub price: f64,  // in sats / FC       // in sats / FC
    pub network: Network,
}

// Quick Reference for FatCrab Order to n3xB Order Maker/Taker Obligation
// FatCrab Buy Orders - Amount in FC, Price in BTC(sats)/FC
// n3xB Orders - Maker Obligation BTC, Amount in sats, receives FCs. Taker Obligation FC, Limit rate in #FC/#Sats = 1/Price
// FatCrab Sell Orders - Amount in FC, Price in BTC(sats)/FC
// n3xB Orders - Maker Obligation in FC, Amount in FC, receives sats. Taker Obligation BTC, Limit rate in #Sats/#FC = Price

impl FatCrabOrder {
    pub fn n3xb_network(&self) -> BitcoinNetwork {
        match self.network {
            Network::Bitcoin => BitcoinNetwork::Mainnet,
            Network::Testnet => BitcoinNetwork::Testnet,
            Network::Regtest => BitcoinNetwork::Regtest,
            Network::Signet => BitcoinNetwork::Signet,
            _ => panic!("FatCrabOrder::bitcoin_network_to_n3xb() - Unexpected Bitcoin Network"),
        }
    }

    pub(crate) fn from_n3xb_order(order: Order, network: Network) -> Result<Self, FatCrabError> {
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

                        // Limit rate is BTC of Taker over FC of Maker. Aka price
                        price = Some(order.taker_obligation.content.limit_rate.unwrap());
                    }
                }
                ObligationKind::Bitcoin(n3xb_network, settlement) => {
                    if !Self::bitcoin_network_matches(n3xb_network, network) {
                        return Err(FatCrabError::Simple {
                            description: "FatCrabOrder::from() - Unexpected Bitcoin Network"
                                .to_string(),
                        });
                    }

                    if let Some(settlement) = settlement {
                        if settlement == BitcoinSettlementMethod::Onchain {
                            intended_order_kind = Some(FatCrabOrderType::Buy);
                            let bitcoin_sat_amount = order.maker_obligation.content.amount;
                            let limit_rate = order.taker_obligation.content.limit_rate.unwrap();

                            // Limit rate is FC of Taker over BTC of Maker. Inverse of price
                            price = Some(1.0 / limit_rate);
                            amount = Some(bitcoin_sat_amount * limit_rate);
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
                ObligationKind::Bitcoin(_network, settlement) => {
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
            return Ok(Self {
                order_type: fatcrab_order_kind,
                trade_uuid: order.trade_uuid,
                amount,
                price,
                network,
            });
        } else {
            Err(FatCrabError::Simple {
                description: "FatCrabOrder::from() - Could not determine order type".to_string(),
            })
        }
    }

    fn bitcoin_network_matches(n3xb_network: BitcoinNetwork, bitcoin_network: Network) -> bool {
        match n3xb_network {
            BitcoinNetwork::Mainnet => bitcoin_network == Network::Bitcoin,
            BitcoinNetwork::Testnet => bitcoin_network == Network::Testnet,
            BitcoinNetwork::Regtest => bitcoin_network == Network::Regtest,
            BitcoinNetwork::Signet => bitcoin_network == Network::Signet,
        }
    }
}

impl Into<Order> for FatCrabOrder {
    fn into(self) -> Order {
        let mut builder = OrderBuilder::new();

        match self.order_type {
            FatCrabOrderType::Buy => {
                builder.trade_uuid(self.trade_uuid);

                let maker_obligation_kind = ObligationKind::Bitcoin(
                    self.n3xb_network(),
                    Some(BitcoinSettlementMethod::Onchain),
                );
                let maker_obligation_kinds =
                    HashSet::from_iter(vec![maker_obligation_kind].into_iter());
                let maker_obligation_content = MakerObligationContent {
                    amount: self.amount * self.price, // n3xb amount should be in sats for Maker
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

                // Limit is Taker FC over Maker BTC. Inverse of price
                let taker_obligation_content = TakerObligationContent {
                    limit_rate: Some(1.0 / self.price),
                    market_offset_pct: None,
                    market_oracles: None,
                };

                let taker_obligation = TakerObligation {
                    kinds: taker_obligation_kinds,
                    content: taker_obligation_content,
                };

                builder.taker_obligation(taker_obligation);

                let trade_engine_specifics = FatCrabMakeOrderSpecifics {};
                builder.trade_engine_specifics(Box::new(trade_engine_specifics));
            }
            FatCrabOrderType::Sell => {
                builder.trade_uuid(self.trade_uuid);

                let maker_obligation_kind = ObligationKind::Custom("FatCrab".to_string());
                let maker_obligation_kinds =
                    HashSet::from_iter(vec![maker_obligation_kind].into_iter());
                let maker_obligation_content = MakerObligationContent {
                    amount: self.amount, // n3xb amount for Maker selling in FC
                    amount_min: None,
                };

                let maker_obligation = MakerObligation {
                    kinds: maker_obligation_kinds,
                    content: maker_obligation_content,
                };

                builder.maker_obligation(maker_obligation);

                let taker_obligation_kind = ObligationKind::Bitcoin(
                    self.n3xb_network(),
                    Some(BitcoinSettlementMethod::Onchain),
                );
                let taker_obligation_kinds =
                    HashSet::from_iter(vec![taker_obligation_kind].into_iter());
                let taker_obligation_content = TakerObligationContent {
                    limit_rate: Some(self.price), // n3xb limit is BTC of taker / FC of maker. Aka price
                    market_offset_pct: None,
                    market_oracles: None,
                };

                let taker_obligation = TakerObligation {
                    kinds: taker_obligation_kinds,
                    content: taker_obligation_content,
                };

                builder.taker_obligation(taker_obligation);

                let trade_engine_specifics = FatCrabMakeOrderSpecifics {};
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
