use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::RwLock,
};

use crusty_n3xb::{
    common::types::{BitcoinSettlementMethod, ObligationKind},
    manager::Manager,
    order::FilterTag,
};
use secp256k1::{SecretKey, XOnlyPublicKey};
use uuid::Uuid;

use crate::{
    common::FATCRAB_OBLIGATION_CUSTOM_KIND_STRING,
    error::FatCrabError,
    maker::{FatCrabMaker, FatCrabMakerAccess},
    order::{FatCrabOrder, FatCrabOrderType},
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

    pub async fn query_orders(
        &self,
        order_type: FatCrabOrderType,
    ) -> Result<Vec<FatCrabOrder>, FatCrabError> {
        let custom_fatcrab_obligation_kind: ObligationKind =
            ObligationKind::Custom(FATCRAB_OBLIGATION_CUSTOM_KIND_STRING.to_string());
        let bitcoin_onchain_obligation_kind: ObligationKind =
            ObligationKind::Bitcoin(Some(BitcoinSettlementMethod::Onchain));

        let mut filter_tags = Vec::new();
        match order_type {
            FatCrabOrderType::Buy => {
                let maker_obligation_filter = FilterTag::MakerObligations(HashSet::from_iter(
                    vec![bitcoin_onchain_obligation_kind.clone()].into_iter(),
                ));
                let taker_obligation_filter = FilterTag::TakerObligations(HashSet::from_iter(
                    vec![custom_fatcrab_obligation_kind.clone()].into_iter(),
                ));
                filter_tags.push(maker_obligation_filter);
                filter_tags.push(taker_obligation_filter);
            }

            FatCrabOrderType::Sell => {
                let maker_obligation_filter = FilterTag::MakerObligations(HashSet::from_iter(
                    vec![custom_fatcrab_obligation_kind.clone()].into_iter(),
                ));
                let taker_obligation_filter = FilterTag::TakerObligations(HashSet::from_iter(
                    vec![bitcoin_onchain_obligation_kind.clone()].into_iter(),
                ));
                filter_tags.push(maker_obligation_filter);
                filter_tags.push(taker_obligation_filter);
            }
        }

        let n3xb_orders = self.n3xb_manager.query_orders(filter_tags).await?;
        let order: Vec<FatCrabOrder> = n3xb_orders
            .into_iter()
            .map(|envelope| FatCrabOrder::from_n3xb_order(envelope.order).unwrap())
            .collect();

        Ok(order)
    }

    pub fn take_order(&self) -> FatCrabTakerAccess {
        println!("Taking order");
        FatCrabTakerAccess {}
    }

    pub fn shutdown(self) {}
}
