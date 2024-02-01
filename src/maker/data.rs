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
    offer::FatCrabOfferEnvelope,
    peer::FatCrabPeerEnvelope,
    persist::std::Persister,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FatCrabMakerBuyDataStore {
    trade_uuid: Uuid,
    fatcrab_rx_addr: String,
    btc_funds_id: Uuid,
    peer_btc_addr: Option<String>,
    offer_envelopes: Vec<FatCrabOfferEnvelope>,
    peer_envelope: Option<FatCrabPeerEnvelope>,
    trade_completed: bool,
}

#[typetag::serde(name = "fatcrab_maker_buy_data")]
impl SerdeGenericTrait for FatCrabMakerBuyDataStore {
    fn any_ref(&self) -> &dyn std::any::Any {
        self
    }
}

pub(crate) struct FatCrabMakerBuyData {
    store: Arc<RwLock<FatCrabMakerBuyDataStore>>,
    persister: Persister,
    network: Network,
}

impl FatCrabMakerBuyData {
    pub(crate) fn new(
        trade_uuid: Uuid,
        network: Network,
        fatcrab_rx_addr: impl AsRef<str>,
        btc_funds_id: Uuid,
        dir_path: impl AsRef<Path>,
    ) -> Self {
        let data_path = dir_path.as_ref().join(format!("{}.json", trade_uuid));

        let store = FatCrabMakerBuyDataStore {
            trade_uuid,
            fatcrab_rx_addr: fatcrab_rx_addr.as_ref().to_owned(),
            btc_funds_id,
            peer_btc_addr: None,
            offer_envelopes: Vec::new(),
            peer_envelope: None,
            trade_completed: false,
        };

        let store: Arc<RwLock<FatCrabMakerBuyDataStore>> = Arc::new(RwLock::new(store));
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
        let store = serde_json::from_str::<FatCrabMakerBuyDataStore>(&json)?;
        let trade_uuid = store.trade_uuid;

        let store: Arc<RwLock<FatCrabMakerBuyDataStore>> = Arc::new(RwLock::new(store));
        let generic_store: Arc<RwLock<dyn SerdeGenericTrait + 'static>> = store.clone();

        let data = Self {
            store,
            persister: Persister::new(generic_store, data_path),
            network,
        };
        Ok((trade_uuid, data))
    }

    fn read_store(&self) -> RwLockReadGuard<'_, FatCrabMakerBuyDataStore> {
        match self.store.read() {
            Ok(store) => store,
            Err(error) => {
                panic!("Error reading store - {}", error);
            }
        }
    }

    fn write_store(&self) -> RwLockWriteGuard<'_, FatCrabMakerBuyDataStore> {
        match self.store.write() {
            Ok(store) => store,
            Err(error) => {
                panic!("Error writing store - {}", error);
            }
        }
    }

    pub(crate) fn fatcrab_rx_addr(&self) -> String {
        self.read_store().fatcrab_rx_addr.to_owned()
    }

    pub(crate) fn btc_funds_id(&self) -> Uuid {
        self.read_store().btc_funds_id.to_owned()
    }

    pub(crate) fn peer_btc_addr(&self) -> Option<Address> {
        match &self.read_store().peer_btc_addr {
            Some(addr_string) => {
                let address = parse_address(addr_string, self.network);
                Some(address)
            }
            None => None,
        }
    }

    pub(crate) fn offer_envelopes(&self) -> Vec<FatCrabOfferEnvelope> {
        self.read_store().offer_envelopes.to_owned()
    }

    pub(crate) fn peer_envelope(&self) -> Option<FatCrabPeerEnvelope> {
        self.read_store().peer_envelope.to_owned()
    }

    pub(crate) fn trade_completed(&self) -> bool {
        self.read_store().trade_completed
    }

    pub(crate) fn set_peer_btc_addr(&self, addr: Address) {
        self.write_store().peer_btc_addr = Some(addr.to_string());
        self.persister.queue();
    }

    pub(crate) fn insert_offer_envelope(&self, envelope: FatCrabOfferEnvelope) {
        self.write_store().offer_envelopes.push(envelope);
        self.persister.queue();
    }

    pub(crate) fn set_peer_envelope(&self, envelope: FatCrabPeerEnvelope) {
        self.write_store().peer_envelope = Some(envelope);
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
struct FatCrabMakerSellDataStore {
    trade_uuid: Uuid,
    btc_rx_addr: String,
    peer_btc_txid: Option<Txid>,
    offer_envelopes: Vec<FatCrabOfferEnvelope>,
    peer_envelope: Option<FatCrabPeerEnvelope>,
    trade_completed: bool,
}

#[typetag::serde(name = "fatcrab_maker_sell_data")]
impl SerdeGenericTrait for FatCrabMakerSellDataStore {
    fn any_ref(&self) -> &dyn std::any::Any {
        self
    }
}

pub(crate) struct FatCrabMakerSellData {
    store: Arc<RwLock<FatCrabMakerSellDataStore>>,
    persister: Persister,
    network: Network,
}

impl FatCrabMakerSellData {
    pub(crate) fn new(
        trade_uuid: Uuid,
        network: Network,
        btc_rx_addr: Address,
        dir_path: impl AsRef<Path>,
    ) -> Self {
        let data_path = dir_path.as_ref().join(format!("{}.json", trade_uuid));

        let store = FatCrabMakerSellDataStore {
            trade_uuid,
            btc_rx_addr: btc_rx_addr.to_string(),
            peer_btc_txid: None,
            offer_envelopes: Vec::new(),
            peer_envelope: None,
            trade_completed: false,
        };

        let store: Arc<RwLock<FatCrabMakerSellDataStore>> = Arc::new(RwLock::new(store));
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
        let store = serde_json::from_str::<FatCrabMakerSellDataStore>(&json)?;
        let trade_uuid = store.trade_uuid;

        let store: Arc<RwLock<FatCrabMakerSellDataStore>> = Arc::new(RwLock::new(store));
        let generic_store: Arc<RwLock<dyn SerdeGenericTrait + 'static>> = store.clone();

        let data = Self {
            store,
            persister: Persister::new(generic_store, data_path),
            network,
        };
        Ok((trade_uuid, data))
    }

    fn read_store(&self) -> RwLockReadGuard<'_, FatCrabMakerSellDataStore> {
        match self.store.read() {
            Ok(store) => store,
            Err(error) => {
                panic!("Error reading store - {}", error);
            }
        }
    }

    fn write_store(&self) -> RwLockWriteGuard<'_, FatCrabMakerSellDataStore> {
        match self.store.write() {
            Ok(store) => store,
            Err(error) => {
                panic!("Error writing store - {}", error);
            }
        }
    }

    pub(crate) fn btc_rx_addr(&self) -> Address {
        let addr_string = self.read_store().btc_rx_addr.clone();
        parse_address(addr_string, self.network)
    }

    pub(crate) fn peer_btc_txid(&self) -> Option<Txid> {
        self.read_store().peer_btc_txid
    }

    pub(crate) fn offer_envelopes(&self) -> Vec<FatCrabOfferEnvelope> {
        self.read_store().offer_envelopes.to_owned()
    }

    pub(crate) fn peer_envelope(&self) -> Option<FatCrabPeerEnvelope> {
        self.read_store().peer_envelope.to_owned()
    }

    pub(crate) fn trade_completed(&self) -> bool {
        self.read_store().trade_completed
    }

    pub(crate) fn set_peer_btc_txid(&self, txid: Txid) {
        self.write_store().peer_btc_txid = Some(txid);
        self.persister.queue();
    }

    pub(crate) fn insert_offer_envelope(&self, envelope: FatCrabOfferEnvelope) {
        self.write_store().offer_envelopes.push(envelope);
        self.persister.queue();
    }

    pub(crate) fn set_peer_envelope(&self, envelope: FatCrabPeerEnvelope) {
        self.write_store().peer_envelope = Some(envelope);
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
