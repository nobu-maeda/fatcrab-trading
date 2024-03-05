use std::{
    path::Path,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use bitcoin::{Address, Network, Txid};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    common::{parse_address, SerdeGenericTrait},
    error::FatCrabError,
    order::FatCrabOrderEnvelope,
    peer::FatCrabPeerEnvelope,
    persist::std::Persister,
    trade_rsp::FatCrabTradeRspEnvelope,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FatCrabTakerBuyDataStore {
    order_envelope: FatCrabOrderEnvelope,
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
    pub(crate) fn new(
        order_envelope: &FatCrabOrderEnvelope,
        network: Network,
        btc_rx_addr: Address,
        dir_path: impl AsRef<Path>,
    ) -> Self {
        let data_path = dir_path
            .as_ref()
            .join(format!("{}.json", order_envelope.order.trade_uuid));

        let store = FatCrabTakerBuyDataStore {
            order_envelope: order_envelope.to_owned(),
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

    pub(crate) fn restore(
        network: Network,
        data_path: impl AsRef<Path>,
    ) -> Result<(Uuid, Self), FatCrabError> {
        let json = Persister::restore(&data_path)?;
        let store = serde_json::from_str::<FatCrabTakerBuyDataStore>(&json)?;
        let trade_uuid = store.order_envelope.order.trade_uuid;

        let store: Arc<RwLock<FatCrabTakerBuyDataStore>> = Arc::new(RwLock::new(store));
        let generic_store: Arc<RwLock<dyn SerdeGenericTrait + 'static>> = store.clone();

        let data = Self {
            store,
            persister: Persister::new(generic_store, data_path),
            network,
        };

        Ok((trade_uuid, data))
    }

    fn read_store(&self) -> RwLockReadGuard<'_, FatCrabTakerBuyDataStore> {
        match self.store.read() {
            Ok(store) => store,
            Err(error) => {
                panic!("Error reading store - {}", error);
            }
        }
    }

    fn write_store(&self) -> RwLockWriteGuard<'_, FatCrabTakerBuyDataStore> {
        match self.store.write() {
            Ok(store) => store,
            Err(error) => {
                panic!("Error writing store - {}", error);
            }
        }
    }

    pub(crate) fn order_envelope(&self) -> FatCrabOrderEnvelope {
        self.read_store().order_envelope.clone()
    }

    pub(crate) fn btc_rx_addr(&self) -> Address {
        let addr_string = self.read_store().btc_rx_addr.clone();
        parse_address(addr_string, self.network)
    }

    pub(crate) fn peer_btc_txid(&self) -> Option<Txid> {
        self.read_store().peer_btc_txid
    }

    pub(crate) fn trade_rsp_envelope(&self) -> Option<FatCrabTradeRspEnvelope> {
        self.read_store().trade_rsp_envelope.clone()
    }

    pub(crate) fn peer_envelope(&self) -> Option<FatCrabPeerEnvelope> {
        self.read_store().peer_envelope.clone()
    }

    pub(crate) fn trade_completed(&self) -> bool {
        self.read_store().trade_completed
    }

    pub(crate) fn set_peer_btc_txid(&self, txid: Txid) {
        self.write_store().peer_btc_txid = Some(txid);
        self.persister.queue();
    }

    pub(crate) fn set_trade_rsp_envelope(&self, trade_rsp_envelope: FatCrabTradeRspEnvelope) {
        self.write_store().trade_rsp_envelope = Some(trade_rsp_envelope);
        self.persister.queue();
    }

    pub(crate) fn set_peer_envelope(&self, peer_envelope: FatCrabPeerEnvelope) {
        self.write_store().peer_envelope = Some(peer_envelope);
        self.persister.queue();
    }

    pub(crate) fn set_trade_completed(&self) {
        self.write_store().trade_completed = true;
        self.persister.queue();
    }

    pub(crate) fn terminate(self) {
        self.persister.terminate();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FatCrabTakerSellDataStore {
    order_envelope: FatCrabOrderEnvelope,
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
    pub(crate) fn new(
        order_envelope: &FatCrabOrderEnvelope,
        fatcrab_rx_addr: impl AsRef<str>,
        btc_funds_id: Uuid,
        dir_path: impl AsRef<Path>,
    ) -> Self {
        let data_path = dir_path
            .as_ref()
            .join(format!("{}.json", order_envelope.order.trade_uuid));

        let store = FatCrabTakerSellDataStore {
            order_envelope: order_envelope.to_owned(),
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

    pub(crate) fn restore(data_path: impl AsRef<Path>) -> Result<(Uuid, Self), FatCrabError> {
        let json = Persister::restore(&data_path)?;
        let store = serde_json::from_str::<FatCrabTakerSellDataStore>(&json)?;
        let trade_uuid = store.order_envelope.order.trade_uuid;

        let store: Arc<RwLock<FatCrabTakerSellDataStore>> = Arc::new(RwLock::new(store));
        let generic_store: Arc<RwLock<dyn SerdeGenericTrait + 'static>> = store.clone();

        let data = Self {
            store,
            persister: Persister::new(generic_store, data_path),
        };

        Ok((trade_uuid, data))
    }

    fn read_store(&self) -> RwLockReadGuard<'_, FatCrabTakerSellDataStore> {
        match self.store.read() {
            Ok(store) => store,
            Err(error) => {
                panic!("Error reading store - {}", error);
            }
        }
    }

    fn write_store(&self) -> RwLockWriteGuard<'_, FatCrabTakerSellDataStore> {
        match self.store.write() {
            Ok(store) => store,
            Err(error) => {
                panic!("Error writing store - {}", error);
            }
        }
    }

    pub(crate) fn order_envelope(&self) -> FatCrabOrderEnvelope {
        self.read_store().order_envelope.clone()
    }

    pub(crate) fn fatcrab_rx_addr(&self) -> String {
        self.read_store().fatcrab_rx_addr.to_owned()
    }

    pub(crate) fn btc_funds_id(&self) -> Uuid {
        self.read_store().btc_funds_id
    }

    pub(crate) fn peer_envelope(&self) -> Option<FatCrabPeerEnvelope> {
        self.read_store().peer_envelope.clone()
    }

    pub(crate) fn trade_completed(&self) -> bool {
        self.read_store().trade_completed
    }

    pub(crate) fn set_peer_envelope(&self, peer_envelope: FatCrabPeerEnvelope) {
        self.write_store().peer_envelope = Some(peer_envelope);
        self.persister.queue();
    }

    pub(crate) fn set_trade_completed(&self) {
        self.write_store().trade_completed = true;
        self.persister.queue();
    }

    pub(crate) fn terminate(self) {
        self.persister.terminate();
    }
}
