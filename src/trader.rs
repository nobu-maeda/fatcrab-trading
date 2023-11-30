use std::{collections::HashMap, net::SocketAddr, sync::RwLock};

use crusty_n3xb::manager::Manager;
use secp256k1::{SecretKey, XOnlyPublicKey};
use uuid::Uuid;

use crate::{
    error::FatCrabError,
    make_trade::{MakeTrade, MakeTradeAccess},
    take_trade::{TakeTrade, TakeTradeAccess},
    trade_order::TradeOrder,
};

pub struct Trader {
    n3xb_manager: Manager,
    make_trades: RwLock<HashMap<Uuid, MakeTrade>>,
    take_trades: RwLock<HashMap<Uuid, TakeTrade>>,
    make_trade_accessors: RwLock<HashMap<Uuid, MakeTradeAccess>>,
    take_trade_accessors: RwLock<HashMap<Uuid, TakeTradeAccess>>,
}

impl Trader {
    pub async fn new() -> Self {
        let trade_engine_name = "fat-crab-trade-engine";
        Self {
            n3xb_manager: Manager::new(trade_engine_name).await,
            make_trades: RwLock::new(HashMap::new()),
            take_trades: RwLock::new(HashMap::new()),
            make_trade_accessors: RwLock::new(HashMap::new()),
            take_trade_accessors: RwLock::new(HashMap::new()),
        }
    }

    pub async fn new_with_keys(key: SecretKey) -> Self {
        let trade_engine_name = "fat-crab-trade-engine";
        let n3xb_manager = Manager::new_with_keys(key, trade_engine_name).await;
        Self {
            n3xb_manager,
            make_trades: RwLock::new(HashMap::new()),
            take_trades: RwLock::new(HashMap::new()),
            make_trade_accessors: RwLock::new(HashMap::new()),
            take_trade_accessors: RwLock::new(HashMap::new()),
        }
    }

    pub async fn pubkey(&self) -> XOnlyPublicKey {
        self.n3xb_manager.pubkey().await
    }

    pub async fn add_relays(
        &self,
        relays: Vec<(String, Option<SocketAddr>)>,
    ) -> Result<(), FatCrabError> {
        self.n3xb_manager.add_relays(relays, true).await?;
        Ok(())
    }

    pub async fn make_order(&self, trade_order: TradeOrder) -> MakeTradeAccess {
        let maker = self
            .n3xb_manager
            .new_maker(trade_order.clone().into())
            .await
            .unwrap();

        let make_trade = MakeTrade::new(trade_order, maker);
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

    pub fn list_orders(&self) {
        println!("Listing orders");
    }

    pub fn take_order(&self) -> TakeTradeAccess {
        println!("Taking order");
        TakeTradeAccess {}
    }

    pub fn shutdown(self) {}
}
