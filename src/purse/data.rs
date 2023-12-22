use std::{collections::HashMap, path::Path, sync::Arc};

use bitcoin::Network;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    common::{Persister, SerdeGenericTrait},
    error::FatCrabError,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PurseDataStore {
    network: Network,
    height: u32,
    allocated_funds: HashMap<Uuid, u64>,
}

#[typetag::serde(name = "fatcrab_purse_data")]
impl SerdeGenericTrait for PurseDataStore {
    fn any_ref(&self) -> &dyn std::any::Any {
        self
    }
}

pub(crate) struct PurseData {
    store: Arc<RwLock<PurseDataStore>>,
    persister: Persister,
}

impl PurseData {
    pub(crate) fn new(
        pubkey_string: impl AsRef<str>,
        network: Network,
        height: u32,
        dir_path: impl AsRef<Path>,
    ) -> Self {
        let data_path = dir_path
            .as_ref()
            .join(format!("{}.json", pubkey_string.as_ref()));

        let store = PurseDataStore {
            network,
            height,
            allocated_funds: HashMap::new(),
        };

        let store: Arc<RwLock<PurseDataStore>> = Arc::new(RwLock::new(store));
        let generic_store: Arc<RwLock<dyn SerdeGenericTrait + 'static>> = store.clone();
        let persister = Persister::new(generic_store, data_path);
        persister.queue();

        Self { store, persister }
    }

    pub(crate) async fn restore(
        pubkey_string: impl AsRef<str>,
        dir_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let data_path = dir_path
            .as_ref()
            .join(format!("{}.json", pubkey_string.as_ref()));

        let json = Persister::restore(&data_path).await?;
        let store = serde_json::from_str::<PurseDataStore>(&json)?;

        let store: Arc<RwLock<PurseDataStore>> = Arc::new(RwLock::new(store));
        let generic_store: Arc<RwLock<dyn SerdeGenericTrait + 'static>> = store.clone();

        let data = Self {
            store,
            persister: Persister::new(generic_store, dir_path),
        };
        Ok(data)
    }

    pub(crate) async fn network(&self) -> Network {
        self.store.read().await.network
    }

    pub(crate) async fn height(&self) -> u32 {
        self.store.read().await.height
    }

    pub(crate) async fn total_funds_allocated(&self) -> u64 {
        self.store
            .read()
            .await
            .allocated_funds
            .values()
            .sum::<u64>()
    }

    pub(crate) async fn get_allocated_funds(&self, uuid: &Uuid) -> Option<u64> {
        self.store.read().await.allocated_funds.get(uuid).copied()
    }

    pub(crate) async fn allocated_funds(&self, uuid: &Uuid, amount: u64, height: Option<u32>) {
        {
            let mut store = self.store.write().await;
            store.allocated_funds.insert(uuid.to_owned(), amount);

            if let Some(height) = height {
                store.height = height;
            }
        }
        self.persister.queue();
    }

    pub(crate) async fn deallocate_funds(&self, uuid: &Uuid, height: Option<u32>) -> Option<u64> {
        let sats = {
            let mut store = self.store.write().await;
            let sats = store.allocated_funds.remove(uuid);

            if let Some(height) = height {
                store.height = height;
            }
            sats
        };
        self.persister.queue();
        sats
    }

    pub(crate) async fn terminate(self) {
        self.persister.terminate().await;
    }
}
