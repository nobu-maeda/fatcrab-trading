use std::{path::Path, sync::Arc};

use bitcoin::{Address, Network, Txid};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    common::{parse_address, Persister, SerdeGenericTrait},
    error::FatCrabError,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FatCrabMakerBuyDataStore {
    trade_uuid: Uuid,
    fatcrab_rx_addr: String,
    btc_funds_id: Uuid,
    peer_btc_addr: Option<String>,
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
    pub(crate) async fn new(
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
            trade_completed: false,
        };

        let store: Arc<RwLock<FatCrabMakerBuyDataStore>> = Arc::new(RwLock::new(store));
        let generic_store: Arc<RwLock<dyn SerdeGenericTrait + 'static>> = store.clone();
        let persister = Persister::new(generic_store, data_path);

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

    pub(crate) async fn fatcrab_rx_addr(&self) -> String {
        self.store.read().await.fatcrab_rx_addr.to_owned()
    }

    pub(crate) async fn btc_funds_id(&self) -> Uuid {
        self.store.read().await.btc_funds_id.to_owned()
    }

    pub(crate) async fn peer_btc_addr(&self) -> Option<Address> {
        match &self.store.read().await.peer_btc_addr {
            Some(addr_string) => {
                let address = parse_address(addr_string, self.network);
                Some(address)
            }
            None => None,
        }
    }

    pub(crate) async fn trade_completed(&self) -> bool {
        self.store.read().await.trade_completed
    }

    pub(crate) async fn set_peer_btc_addr(&self, addr: Address) {
        self.store.write().await.peer_btc_addr = Some(addr.to_string());
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
struct FatCrabMakerSellDataStore {
    trade_uuid: Uuid,
    btc_rx_addr: String,
    peer_btc_txid: Option<Txid>,
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
    pub(crate) async fn new(
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

    pub(crate) async fn restore(
        network: Network,
        data_path: impl AsRef<Path>,
    ) -> Result<(Uuid, Self), FatCrabError> {
        let json = Persister::restore(&data_path).await?;
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

    pub(crate) async fn btc_rx_addr(&self) -> Address {
        let addr_string = self.store.read().await.btc_rx_addr.clone();
        parse_address(addr_string, self.network)
    }

    pub(crate) async fn peer_btc_txid(&self) -> Option<Txid> {
        self.store.read().await.peer_btc_txid
    }

    pub(crate) async fn trade_completed(&self) -> bool {
        self.store.read().await.trade_completed
    }

    pub(crate) async fn set_peer_btc_txid(&self, txid: Txid) {
        self.store.write().await.peer_btc_txid = Some(txid);
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
