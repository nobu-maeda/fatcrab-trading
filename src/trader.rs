use std::{collections::HashMap, net::SocketAddr, sync::RwLock};

use crusty_n3xb::{machine::taker::TakerAccess, manager::Manager};
use secp256k1::{SecretKey, XOnlyPublicKey};
use uuid::Uuid;

use crate::{
    error::FatCrabError,
    maker::{FatCrabMaker, FatCrabMakerAccess},
    order::FatCrabOrder,
    taker::{FatCrabTaker, FatCrabTakerAccess},
};

pub struct FatCrabTrader {
    n3xb_manager: Manager,
    makers: RwLock<HashMap<Uuid, FatCrabMaker>>,
    takers: RwLock<HashMap<Uuid, FatCrabTaker>>,
    maker_accessors: RwLock<HashMap<Uuid, FatCrabMakerAccess>>,
    taker_accessors: RwLock<HashMap<Uuid, FatCrabTakerAccess>>,
}

impl FatCrabTrader {
    pub async fn new() -> Self {
        let trade_engine_name = "fat-crab-trade-engine";
        Self {
            n3xb_manager: Manager::new(trade_engine_name).await,
            makers: RwLock::new(HashMap::new()),
            takers: RwLock::new(HashMap::new()),
            maker_accessors: RwLock::new(HashMap::new()),
            taker_accessors: RwLock::new(HashMap::new()),
        }
    }

    pub async fn new_with_keys(key: SecretKey) -> Self {
        let trade_engine_name = "fat-crab-trade-engine";
        let n3xb_manager = Manager::new_with_keys(key, trade_engine_name).await;
        Self {
            n3xb_manager,
            makers: RwLock::new(HashMap::new()),
            takers: RwLock::new(HashMap::new()),
            maker_accessors: RwLock::new(HashMap::new()),
            taker_accessors: RwLock::new(HashMap::new()),
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

    pub async fn make_order(&self, order: FatCrabOrder) -> FatCrabMakerAccess {
        let n3xb_maker = self
            .n3xb_manager
            .new_maker(order.clone().into())
            .await
            .unwrap();

        let maker = FatCrabMaker::new(order, n3xb_maker).await;
        let maker_accessor = maker.new_accessor();
        let maker_return_accessor = maker.new_accessor();
        let maker_id = Uuid::new_v4();

        self.makers.write().unwrap().insert(maker_id, maker);

        self.maker_accessors
            .write()
            .unwrap()
            .insert(maker_id, maker_accessor);

        maker_return_accessor
    }

    pub fn list_orders(&self) {
        println!("Listing orders");
    }

    pub fn take_order(&self) -> FatCrabTakerAccess {
        println!("Taking order");
        FatCrabTakerAccess {}
    }

    pub fn shutdown(self) {}
}
