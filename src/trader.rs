use std::{collections::HashMap, sync::RwLock};

use crusty_n3xb::manager::Manager;
use uuid::Uuid;

use crate::{
    make_trade::{MakeTrade, MakeTradeAccess},
    take_trade::{TakeTrade, TakeTradeAccess},
};

pub struct Trader {
    n3xB_manager: Manager,
    make_trades: RwLock<HashMap<Uuid, MakeTrade>>,
    take_trades: RwLock<HashMap<Uuid, TakeTrade>>,
    make_trade_accessors: RwLock<HashMap<Uuid, MakeTradeAccess>>,
    take_trade_accessors: RwLock<HashMap<Uuid, TakeTradeAccess>>,
}

impl Trader {
    pub async fn new() -> Self {
        let trade_engine_name = "fat-crab-trade-engine";
        Self {
            n3xB_manager: Manager::new(trade_engine_name).await,
            make_trades: RwLock::new(HashMap::new()),
            take_trades: RwLock::new(HashMap::new()),
            make_trade_accessors: RwLock::new(HashMap::new()),
            take_trade_accessors: RwLock::new(HashMap::new()),
        }
    }

    pub fn list_orders(&self) {
        println!("Listing orders");
    }

    pub fn make_order(&self) -> MakeTradeAccess {
        println!("Making order");
        let make_trade = MakeTrade::new();
        let make_trade_accessor = make_trade.new_accessor();
        let make_trade_return_accessor = make_trade_accessor.clone();
        let make_trade_id = Uuid::new_v4();

        self.make_trades
            .write()
            .unwrap()
            .insert(make_trade_id, make_trade);

        self.make_trade_accessors
            .write()
            .unwrap()
            .insert(make_trade_id, make_trade_accessor);

        make_trade_return_accessor
    }

    pub fn take_order(&self) -> TakeTrade {
        println!("Taking order");
        TakeTrade {}
    }
}
