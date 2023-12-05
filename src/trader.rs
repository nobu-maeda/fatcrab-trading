use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::RwLock,
};

use bdk::{
    bitcoin::{bip32::ExtendedPrivKey, Network},
    database::MemoryDatabase,
    template::Bip84,
    KeychainKind, Wallet,
};
use crusty_n3xb::{
    common::types::{BitcoinSettlementMethod, ObligationKind},
    manager::Manager,
    order::FilterTag,
};
use secp256k1::{rand, SecretKey, XOnlyPublicKey};
use uuid::Uuid;

use crate::{
    common::FATCRAB_OBLIGATION_CUSTOM_KIND_STRING,
    error::FatCrabError,
    maker::{FatCrabMaker, FatCrabMakerAccess},
    offer::FatCrabOffer,
    order::{FatCrabOrder, FatCrabOrderEnvelope, FatCrabOrderType},
    taker::{FatCrabTaker, FatCrabTakerAccess},
};

pub struct FatCrabTrader {
    secret_key: SecretKey,
    n3xb_manager: Manager,
    wallet: Wallet<MemoryDatabase>,
    makers: RwLock<HashMap<Uuid, FatCrabMaker>>,
    takers: RwLock<HashMap<Uuid, FatCrabTaker>>,
    maker_accessors: RwLock<HashMap<Uuid, FatCrabMakerAccess>>,
    taker_accessors: RwLock<HashMap<Uuid, FatCrabTakerAccess>>,
}

impl FatCrabTrader {
    fn create_wallet(key: SecretKey, network: Network) -> Wallet<MemoryDatabase> {
        let secret_bytes = key.secret_bytes();
        let xprv = ExtendedPrivKey::new_master(network, &secret_bytes).unwrap();

        Wallet::new(
            Bip84(xprv, KeychainKind::External),
            Some(Bip84(xprv, KeychainKind::Internal)),
            network,
            MemoryDatabase::default(),
        )
        .unwrap()
    }

    pub async fn new() -> Self {
        let secret_key = SecretKey::new(&mut rand::thread_rng());
        Self::new_with_keys(secret_key).await
    }

    pub async fn new_with_keys(secret_key: SecretKey) -> Self {
        let trade_engine_name = "fat-crab-trade-engine";
        let n3xb_manager = Manager::new_with_keys(secret_key, trade_engine_name).await;
        let wallet = Self::create_wallet(secret_key, Network::Regtest);

        Self {
            secret_key,
            n3xb_manager,
            wallet,
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

    pub async fn make_order(
        &self,
        order: FatCrabOrder,
        receive_address: impl Into<String>,
    ) -> FatCrabMakerAccess {
        let n3xb_maker = self
            .n3xb_manager
            .new_maker(order.clone().into())
            .await
            .unwrap();

        let trade_uuid = order.trade_uuid.clone();
        let maker = FatCrabMaker::new(order, receive_address, n3xb_maker).await;
        let maker_accessor = maker.new_accessor();
        let maker_return_accessor = maker.new_accessor();

        self.makers
            .write()
            .unwrap()
            .insert(trade_uuid.clone(), maker);

        self.maker_accessors
            .write()
            .unwrap()
            .insert(trade_uuid, maker_accessor);

        maker_return_accessor
    }

    pub async fn query_orders(
        &self,
        order_type: FatCrabOrderType,
    ) -> Result<Vec<FatCrabOrderEnvelope>, FatCrabError> {
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
        let orders: Vec<FatCrabOrderEnvelope> = n3xb_orders
            .into_iter()
            .map(|envelope| FatCrabOrderEnvelope {
                order: FatCrabOrder::from_n3xb_order(envelope.order.clone()).unwrap(),
                envelope,
            })
            .collect();
        Ok(orders)
    }

    pub async fn take_order(
        &self,
        order: FatCrabOrderEnvelope,
        receive_address: impl Into<String>,
    ) -> FatCrabTakerAccess {
        let n3xb_taker = self
            .n3xb_manager
            .new_taker(
                order.envelope.clone(),
                FatCrabOffer::create_n3xb_offer(order.order.clone()),
            )
            .await
            .unwrap();
        n3xb_taker.take_order().await.unwrap();

        let trade_uuid = order.order.trade_uuid.clone();
        let taker = FatCrabTaker::new(order, receive_address, n3xb_taker).await;
        let taker_accessor = taker.new_accessor();
        let taker_return_accessor = taker.new_accessor();

        self.takers
            .write()
            .unwrap()
            .insert(trade_uuid.clone(), taker);

        self.taker_accessors
            .write()
            .unwrap()
            .insert(trade_uuid, taker_accessor);

        taker_return_accessor
    }

    pub fn shutdown(self) {}
}
