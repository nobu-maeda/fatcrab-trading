use log::{error, trace};
use std::{any::Any, path::Path, str::FromStr, sync::Arc};

use bitcoin::{Address, Network};
use core_rpc::Auth;

use tokio::{
    io::AsyncWriteExt,
    select,
    sync::{mpsc, RwLock, RwLockReadGuard},
};

use crate::error::FatCrabError;

pub(crate) static FATCRAB_OBLIGATION_CUSTOM_KIND_STRING: &str = "FatCrab";

#[derive(Debug, Clone)]
pub enum BlockchainInfo {
    Electrum {
        url: String,
        network: Network,
    },
    Rpc {
        url: String,
        auth: Auth,
        network: Network,
    },
}

pub fn parse_address(address: impl AsRef<str>, network: Network) -> Address {
    let btc_unchecked_addr = Address::from_str(address.as_ref()).unwrap();
    let btc_addr = match btc_unchecked_addr.require_network(network) {
        Ok(addr) => addr,
        Err(error) => {
            panic!(
                "Address {:?} is not {} - {}",
                address.as_ref(),
                network,
                error.to_string()
            );
        }
    };
    btc_addr
}

#[typetag::serde(tag = "type")]
pub trait SerdeGenericTrait: Send + Sync {
    fn any_ref(&self) -> &dyn Any;
}

impl dyn SerdeGenericTrait {
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.any_ref().downcast_ref()
    }
}

enum PersisterMsg {
    Persist,
    Close,
}

pub(crate) struct Persister {
    persist_tx: mpsc::Sender<PersisterMsg>,
    task_handle: tokio::task::JoinHandle<()>,
}

impl Persister {
    pub(crate) async fn restore(data_path: impl AsRef<Path>) -> Result<String, FatCrabError> {
        let json = tokio::fs::read_to_string(data_path.as_ref()).await?;
        Ok(json)
    }

    pub(crate) fn new(
        store: Arc<RwLock<dyn SerdeGenericTrait>>,
        data_path: impl AsRef<Path>,
    ) -> Self {
        let (persist_tx, task_handle) = Self::setup_persistence(store, data_path);

        Self {
            persist_tx,
            task_handle,
        }
    }

    fn setup_persistence(
        store: Arc<RwLock<dyn SerdeGenericTrait>>,
        data_path: impl AsRef<Path>,
    ) -> (mpsc::Sender<PersisterMsg>, tokio::task::JoinHandle<()>) {
        let data_path_buf = data_path.as_ref().to_path_buf();

        let (persist_tx, mut persist_rx) = mpsc::channel(1);
        let task_handle = tokio::spawn(async move {
            let data_path = data_path_buf.clone();
            loop {
                select! {
                    Some(msg) = persist_rx.recv() => {
                        match msg {
                            PersisterMsg::Persist => {
                                let store = store.read().await;
                                if let Some(error) = Self::persist(store, &data_path).await.err() {
                                    error!("Error persisting data to path {} - {}", data_path.display().to_string(), error);
                                }
                            }
                            PersisterMsg::Close => {
                                break;
                            }
                        }
                    },
                    else => break,
                }
            }
        });
        (persist_tx, task_handle)
    }

    async fn persist(
        store: RwLockReadGuard<'_, dyn SerdeGenericTrait>,
        data_path: impl AsRef<Path>,
    ) -> Result<(), FatCrabError> {
        let json = serde_json::to_string(&*store).unwrap();
        let mut file = tokio::fs::File::create(data_path.as_ref()).await?;
        file.write_all(json.as_bytes()).await?;
        file.sync_all().await?;
        Ok(())
    }

    pub(crate) fn queue(&self) {
        match self.persist_tx.try_send(PersisterMsg::Persist) {
            Ok(_) => {}
            Err(error) => match error {
                mpsc::error::TrySendError::Full(_) => {
                    trace!("Persistence channel full")
                }
                mpsc::error::TrySendError::Closed(_) => {
                    error!("Persistence channel closed")
                }
            },
        }
    }

    pub(crate) async fn terminate(self) {
        self.persist_tx.send(PersisterMsg::Close).await.unwrap();
        self.task_handle.await.unwrap();
    }
}
