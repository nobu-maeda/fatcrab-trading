use log::{error, trace};
use std::{path::Path, sync::Arc};

use tokio::{
    fs, select,
    sync::{mpsc, RwLock, RwLockReadGuard},
};

use crate::{common::SerdeGenericTrait, error::FatCrabError};

use super::PersisterMsg;

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
        fs::write(data_path, json).await?;
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
