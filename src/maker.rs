use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crusty_n3xb::machine::maker::MakerAccess;

use crate::error::FatCrabError;
use crate::order::FatCrabOrder;

#[derive(Clone)]
pub struct FatCrabMakerAccess {
    tx: mpsc::Sender<FatCrabMakerRequest>,
}

impl FatCrabMakerAccess {
    async fn new(tx: mpsc::Sender<FatCrabMakerRequest>) -> Self {
        Self { tx }
    }

    pub async fn register_notif_tx(&self, tx: mpsc::Sender<String>) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::RegisterNotifTx { tx, rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }
}

pub(crate) struct FatCrabMaker {
    tx: mpsc::Sender<FatCrabMakerRequest>,
    task_handle: tokio::task::JoinHandle<()>,
}

impl FatCrabMaker {
    const MAKE_TRADE_REQUEST_CHANNEL_SIZE: usize = 10;

    pub(crate) async fn new(order: FatCrabOrder, n3xb_maker: MakerAccess) -> Self {
        let (tx, rx) = mpsc::channel::<FatCrabMakerRequest>(Self::MAKE_TRADE_REQUEST_CHANNEL_SIZE);
        let mut actor = FatCrabMakerActor::new(rx, order, n3xb_maker).await;
        let task_handle = tokio::spawn(async move { actor.run().await });
        Self { tx, task_handle }
    }

    pub(crate) fn new_accessor(&self) -> FatCrabMakerAccess {
        FatCrabMakerAccess {
            tx: self.tx.clone(),
        }
    }
}

enum FatCrabMakerRequest {
    RegisterNotifTx {
        tx: mpsc::Sender<String>,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
}

struct FatCrabMakerActor {
    rx: mpsc::Receiver<FatCrabMakerRequest>,
    order: FatCrabOrder,
    n3xb_maker: MakerAccess,
}

impl FatCrabMakerActor {
    async fn new(
        rx: mpsc::Receiver<FatCrabMakerRequest>,
        order: FatCrabOrder,
        n3xb_maker: MakerAccess,
    ) -> Self {
        n3xb_maker.post_new_order().await.unwrap();

        Self {
            rx,
            order,
            n3xb_maker,
        }
    }

    async fn run(&mut self) {
        while let Some(req) = self.rx.recv().await {
            match req {
                FatCrabMakerRequest::RegisterNotifTx { tx, rsp_tx } => {
                    tx.send(Uuid::new_v4().to_string()).await.unwrap();
                    rsp_tx.send(Ok(())).unwrap();
                }
            }
        }
    }
}
