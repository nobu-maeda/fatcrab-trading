use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, RwLockReadGuard, RwLockWriteGuard},
};

use bitcoin::Network;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use uuid::Uuid;

use crate::{common::SerdeGenericTrait, error::FatCrabError, persist::std::Persister};

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

        let mut store = PurseDataStore {
            network,
            height,
            allocated_funds: HashMap::new(),
        };

        if data_path.exists() {
            match Self::restore(&data_path) {
                Ok(restored_data) => {
                    store = restored_data;
                }
                Err(error) => {
                    panic!(
                        "Purse w/ Pubkey {} - Error restoring data from path {}: {}",
                        pubkey_string.as_ref().to_string(),
                        data_path.display().to_string(),
                        error
                    );
                }
            }
        }

        let store: Arc<RwLock<PurseDataStore>> = Arc::new(RwLock::new(store));
        let generic_store: Arc<RwLock<dyn SerdeGenericTrait + 'static>> = store.clone();
        let persister = Persister::new(generic_store, &data_path);
        persister.queue();

        Self { store, persister }
    }

    fn restore(data_path: impl AsRef<Path>) -> Result<PurseDataStore, FatCrabError> {
        let json = Persister::restore(&data_path)?;
        let store = serde_json::from_str::<PurseDataStore>(&json)?;
        Ok(store)
    }

    fn read_store(&self) -> RwLockReadGuard<'_, PurseDataStore> {
        match self.store.read() {
            Ok(store) => store,
            Err(error) => {
                panic!("Error reading store - {}", error);
            }
        }
    }

    fn write_store(&self) -> RwLockWriteGuard<'_, PurseDataStore> {
        match self.store.write() {
            Ok(store) => store,
            Err(error) => {
                panic!("Error writing store - {}", error);
            }
        }
    }

    pub(crate) fn network(&self) -> Network {
        self.read_store().network
    }

    pub(crate) fn height(&self) -> u32 {
        self.read_store().height
    }

    pub(crate) fn total_funds_allocated(&self) -> u64 {
        self.read_store().allocated_funds.values().sum::<u64>()
    }

    pub(crate) fn get_allocated_funds(&self, uuid: &Uuid) -> Option<u64> {
        self.read_store().allocated_funds.get(uuid).copied()
    }

    pub(crate) fn allocate_funds(&self, uuid: &Uuid, amount: u64, height: Option<u32>) {
        {
            let mut store = self.write_store();
            store.allocated_funds.insert(uuid.to_owned(), amount);

            if let Some(height) = height {
                store.height = height;
            }
        }
        self.persister.queue();
    }

    pub(crate) fn deallocate_funds(&self, uuid: &Uuid, height: Option<u32>) -> Option<u64> {
        let sats = {
            let mut store = self.write_store();
            let sats = store.allocated_funds.remove(uuid);

            if let Some(height) = height {
                store.height = height;
            }
            sats
        };
        self.persister.queue();
        sats
    }

    pub(crate) fn terminate(self) {
        self.persister.terminate();
    }
}
