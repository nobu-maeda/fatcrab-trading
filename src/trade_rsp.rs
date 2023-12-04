use std::any::Any;

use crusty_n3xb::{
    common::types::SerdeGenericTrait,
    trade_rsp::{TradeResponse, TradeResponseEnvelope, TradeResponseStatus},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct FatCrabMakeTradeRspSpecifics {
    pub(crate) receive_address: String,
}

#[typetag::serde(name = "fatcrab_make_trade_rsp_specifics")]
impl SerdeGenericTrait for FatCrabMakeTradeRspSpecifics {
    fn any_ref(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug, Clone)]
pub struct FatCrabTradeRspEnvelope {
    pub trade_rsp: FatCrabTradeRsp,
    pub(crate) envelope: TradeResponseEnvelope,
}

#[derive(Debug, Clone)]
pub enum FatCrabTradeRsp {
    Accept(String),
    Reject,
}

impl FatCrabTradeRsp {
    pub(crate) fn from_n3xb_trade_rsp(trade_rsp: TradeResponse) -> Self {
        let trade_rsp_specifics = trade_rsp
            .trade_engine_specifics
            .downcast_ref::<FatCrabMakeTradeRspSpecifics>()
            .unwrap();
        match trade_rsp.trade_response {
            TradeResponseStatus::Accepted => {
                FatCrabTradeRsp::Accept(trade_rsp_specifics.receive_address.clone())
            }
            _ => FatCrabTradeRsp::Reject,
        }
    }
}
