use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::error::FatCrabError;

#[derive(Clone)]
pub struct MakeTradeAccess {
    tx: mpsc::Sender<MakeTradeRequest>,
}

impl MakeTradeAccess {
    async fn new(tx: mpsc::Sender<MakeTradeRequest>) -> Self {
        Self { tx }
    }

    pub async fn register_notif_callback(&self, callback: fn(String)) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(MakeTradeRequest::RegisterNotifCallback { callback, rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub async fn register_notif_tx(&self, tx: mpsc::Sender<String>) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(MakeTradeRequest::RegisterNotifTx { tx, rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }
}

pub struct MakeTrade {
    tx: mpsc::Sender<MakeTradeRequest>,
    task_handle: tokio::task::JoinHandle<()>,
}

impl MakeTrade {
    const MAKE_TRADE_REQUEST_CHANNEL_SIZE: usize = 10;

    pub(crate) fn new() -> Self {
        let (tx, rx) = mpsc::channel::<MakeTradeRequest>(Self::MAKE_TRADE_REQUEST_CHANNEL_SIZE);
        let mut actor = MakeTradeActor::new(rx);
        let task_handle = tokio::spawn(async move { actor.run().await });
        Self { tx, task_handle }
    }

    pub(crate) fn new_accessor(&self) -> MakeTradeAccess {
        MakeTradeAccess {
            tx: self.tx.clone(),
        }
    }
}

enum MakeTradeRequest {
    RegisterNotifCallback {
        callback: fn(String),
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    RegisterNotifTx {
        tx: mpsc::Sender<String>,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
}

struct MakeTradeActor {
    rx: mpsc::Receiver<MakeTradeRequest>,
}

impl MakeTradeActor {
    fn new(rx: mpsc::Receiver<MakeTradeRequest>) -> Self {
        Self { rx }
    }

    async fn run(&mut self) {
        while let Some(req) = self.rx.recv().await {
            match req {
                MakeTradeRequest::RegisterNotifCallback { callback, rsp_tx } => {
                    callback(Uuid::new_v4().to_string());
                    rsp_tx.send(Ok(())).unwrap();
                }
                MakeTradeRequest::RegisterNotifTx { tx, rsp_tx } => {
                    tx.send(Uuid::new_v4().to_string()).await.unwrap();
                    rsp_tx.send(Ok(())).unwrap();
                }
            }
        }
    }
}
