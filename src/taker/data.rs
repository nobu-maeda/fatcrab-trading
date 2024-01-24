use std::{path::Path, sync::Arc};

use bitcoin::{Address, Network, Txid};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    common::{parse_address, SerdeGenericTrait},
    error::FatCrabError,
    peer::FatCrabPeerEnvelope,
    persist::tokio::Persister,
    trade_rsp::FatCrabTradeRspEnvelope,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FatCrabTakerBuyDataStore {
    trade_uuid: Uuid,
    btc_rx_addr: String,
    peer_btc_txid: Option<Txid>,
    trade_rsp_envelope: Option<FatCrabTradeRspEnvelope>,
    peer_envelope: Option<FatCrabPeerEnvelope>,
    trade_completed: bool,
}

#[typetag::serde(name = "fatcrab_taker_buy_data")]
impl SerdeGenericTrait for FatCrabTakerBuyDataStore {
    fn any_ref(&self) -> &dyn std::any::Any {
        self
    }
}

pub(crate) struct FatCrabTakerBuyData {
    store: Arc<RwLock<FatCrabTakerBuyDataStore>>,
    persister: Persister,
    network: Network,
}

impl FatCrabTakerBuyData {
    pub(crate) async fn new(
        trade_uuid: Uuid,
        network: Network,
        btc_rx_addr: Address,
        dir_path: impl AsRef<Path>,
    ) -> Self {
        let data_path = dir_path.as_ref().join(format!("{}.json", trade_uuid));

        let store = FatCrabTakerBuyDataStore {
            trade_uuid,
            btc_rx_addr: btc_rx_addr.to_string(),
            peer_btc_txid: None,
            trade_rsp_envelope: None,
            peer_envelope: None,
            trade_completed: false,
        };

        let store: Arc<RwLock<FatCrabTakerBuyDataStore>> = Arc::new(RwLock::new(store));
        let generic_store: Arc<RwLock<dyn SerdeGenericTrait + 'static>> = store.clone();
        let persister = Persister::new(generic_store, data_path);
        persister.queue();

        Self {
            store,
            persister,
            network,
        }
    }

    pub(crate) async fn restore(
        network: Network,
        data_path: impl AsRef<Path>,
    ) -> Result<(Uuid, Self), FatCrabError> {
        let json = Persister::restore(&data_path).await?;
        let store = serde_json::from_str::<FatCrabTakerBuyDataStore>(&json)?;
        let trade_uuid = store.trade_uuid;

        let store: Arc<RwLock<FatCrabTakerBuyDataStore>> = Arc::new(RwLock::new(store));
        let generic_store: Arc<RwLock<dyn SerdeGenericTrait + 'static>> = store.clone();

        let data = Self {
            store,
            persister: Persister::new(generic_store, data_path),
            network,
        };

        Ok((trade_uuid, data))
    }

    pub(crate) async fn btc_rx_addr(&self) -> Address {
        let addr_string = self.store.read().await.btc_rx_addr.clone();
        parse_address(addr_string, self.network)
    }

    pub(crate) async fn peer_btc_txid(&self) -> Option<Txid> {
        self.store.read().await.peer_btc_txid
    }

    pub(crate) async fn trade_rsp_envelope(&self) -> Option<FatCrabTradeRspEnvelope> {
        self.store.read().await.trade_rsp_envelope.clone()
    }

    pub(crate) async fn peer_envelope(&self) -> Option<FatCrabPeerEnvelope> {
        self.store.read().await.peer_envelope.clone()
    }

    pub(crate) async fn trade_completed(&self) -> bool {
        self.store.read().await.trade_completed
    }

    pub(crate) async fn set_peer_btc_txid(&self, txid: Txid) {
        self.store.write().await.peer_btc_txid = Some(txid);
        self.persister.queue();
    }

    pub(crate) async fn set_trade_rsp_envelope(&self, trade_rsp_envelope: FatCrabTradeRspEnvelope) {
        self.store.write().await.trade_rsp_envelope = Some(trade_rsp_envelope);
        self.persister.queue();
    }

    pub(crate) async fn set_peer_envelope(&self, peer_envelope: FatCrabPeerEnvelope) {
        self.store.write().await.peer_envelope = Some(peer_envelope);
        self.persister.queue();
    }

    pub(crate) async fn set_trade_completed(&self) {
        self.store.write().await.trade_completed = true;
        self.persister.queue();
    }

    pub(crate) async fn terminate(self) {
        self.persister.terminate().await;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FatCrabTakerSellDataStore {
    trade_uuid: Uuid,
    fatcrab_rx_addr: String,
    btc_funds_id: Uuid,
    peer_envelope: Option<FatCrabPeerEnvelope>,
    trade_completed: bool,
}

#[typetag::serde(name = "fatcrab_taker_sell_data")]
impl SerdeGenericTrait for FatCrabTakerSellDataStore {
    fn any_ref(&self) -> &dyn std::any::Any {
        self
    }
}

pub(crate) struct FatCrabTakerSellData {
    store: Arc<RwLock<FatCrabTakerSellDataStore>>,
    persister: Persister,
}

impl FatCrabTakerSellData {
    pub(crate) async fn new(
        trade_uuid: Uuid,
        fatcrab_rx_addr: impl AsRef<str>,
        btc_funds_id: Uuid,
        dir_path: impl AsRef<Path>,
    ) -> Self {
        let data_path = dir_path.as_ref().join(format!("{}.json", trade_uuid));

        let store = FatCrabTakerSellDataStore {
            trade_uuid,
            fatcrab_rx_addr: fatcrab_rx_addr.as_ref().to_owned(),
            btc_funds_id,
            peer_envelope: None,
            trade_completed: false,
        };

        let store: Arc<RwLock<FatCrabTakerSellDataStore>> = Arc::new(RwLock::new(store));
        let generic_store: Arc<RwLock<dyn SerdeGenericTrait + 'static>> = store.clone();
        let persister = Persister::new(generic_store, data_path);
        persister.queue();

        Self { store, persister }
    }

    pub(crate) async fn restore(data_path: impl AsRef<Path>) -> Result<(Uuid, Self), FatCrabError> {
        let json = Persister::restore(&data_path).await?;
        let store = serde_json::from_str::<FatCrabTakerSellDataStore>(&json)?;
        let trade_uuid = store.trade_uuid;

        let store: Arc<RwLock<FatCrabTakerSellDataStore>> = Arc::new(RwLock::new(store));
        let generic_store: Arc<RwLock<dyn SerdeGenericTrait + 'static>> = store.clone();

        let data = Self {
            store,
            persister: Persister::new(generic_store, data_path),
        };

        Ok((trade_uuid, data))
    }

    pub(crate) async fn fatcrab_rx_addr(&self) -> String {
        self.store.read().await.fatcrab_rx_addr.to_owned()
    }

    pub(crate) async fn btc_funds_id(&self) -> Uuid {
        self.store.read().await.btc_funds_id
    }

    pub(crate) async fn peer_envelope(&self) -> Option<FatCrabPeerEnvelope> {
        self.store.read().await.peer_envelope.clone()
    }

    pub(crate) async fn trade_completed(&self) -> bool {
        self.store.read().await.trade_completed
    }

    pub(crate) async fn set_peer_envelope(&self, peer_envelope: FatCrabPeerEnvelope) {
        self.store.write().await.peer_envelope = Some(peer_envelope);
        self.persister.queue();
    }

    pub(crate) async fn set_trade_completed(&self) {
        self.store.write().await.trade_completed = true;
        self.persister.queue();
    }

    pub(crate) async fn terminate(self) {
        self.persister.terminate().await;
    }
}
