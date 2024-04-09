use ::tracing::{debug, error, trace};
use std::{
    fs,
    path::Path,
    sync::{
        mpsc::{self, TrySendError},
        Arc, RwLock, RwLockReadGuard,
    },
};

use crate::{common::SerdeGenericTrait, error::FatCrabError};

use super::PersisterMsg;

pub(crate) struct Persister {
    persist_tx: mpsc::SyncSender<PersisterMsg>,
    task_handle: std::thread::JoinHandle<()>,
}

impl Persister {
    pub(crate) fn restore(data_path: impl AsRef<Path>) -> Result<String, FatCrabError> {
        let json: String = std::fs::read_to_string(data_path.as_ref())?;
        debug!(
            "Restored Fatcrab JSON from path {} - {}",
            data_path.as_ref().display().to_string(),
            json
        );
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
    ) -> (mpsc::SyncSender<PersisterMsg>, std::thread::JoinHandle<()>) {
        let data_path_buf = data_path.as_ref().to_path_buf();

        let (persist_tx, persist_rx) = mpsc::sync_channel(1);
        let task_handle = std::thread::spawn(move || {
            let data_path = data_path_buf.clone();
            loop {
                match persist_rx.recv() {
                    Ok(msg) => match msg {
                        PersisterMsg::Persist => {
                            let store = match store.read() {
                                Ok(store) => store,
                                Err(error) => {
                                    error!("Error reading store - {}", error);
                                    continue;
                                }
                            };
                            if let Some(error) = Self::persist(store, &data_path).err() {
                                error!(
                                    "Error persisting data to path {} - {}",
                                    data_path.display().to_string(),
                                    error
                                );
                            }
                        }
                        PersisterMsg::Close => {
                            break;
                        }
                    },
                    Err(err) => {
                        error!("Persistance channel recv Error - {}", err);
                        break;
                    }
                }
            }
        });
        (persist_tx, task_handle)
    }

    fn persist(
        store: RwLockReadGuard<'_, dyn SerdeGenericTrait>,
        data_path: impl AsRef<Path>,
    ) -> Result<(), FatCrabError> {
        let json = serde_json::to_string(&*store)?;

        debug!(
            "Persisting Fatcrab JSON to path {} - {}",
            data_path.as_ref().display().to_string(),
            json
        );

        fs::write(data_path.as_ref(), json)?;
        Ok(())
    }

    pub(crate) fn queue(&self) {
        match self.persist_tx.try_send(PersisterMsg::Persist) {
            Ok(_) => {}
            Err(error) => match error {
                TrySendError::Full(_) => {
                    trace!("Persistence channel full")
                }
                TrySendError::Disconnected(_) => {
                    error!("Persistence channel disconnected")
                }
            },
        }
    }

    pub(crate) fn terminate(self) {
        self.persist_tx.send(PersisterMsg::Close).unwrap();
        if let Some(error) = self.task_handle.join().err() {
            error!("Error terminating persistence thread - {:?}", error);
        }
    }
}
