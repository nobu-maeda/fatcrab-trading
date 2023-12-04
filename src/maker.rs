use tokio::sync::{mpsc, oneshot};

use crusty_n3xb::machine::maker::MakerAccess;

use crate::error::FatCrabError;
use crate::offer::FatCrabOfferEnvelope;
use crate::order::FatCrabOrder;

pub enum FatCrabMakerNotif {
    Offer(FatCrabOfferEnvelope),
}

#[derive(Clone)]
pub struct FatCrabMakerAccess {
    tx: mpsc::Sender<FatCrabMakerRequest>,
}

impl FatCrabMakerAccess {
    pub async fn register_notif_tx(
        &self,
        tx: mpsc::Sender<FatCrabMakerNotif>,
    ) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::RegisterNotifTx { tx, rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub async fn unregister_notif_tx(&self) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::UnregisterNotifTx { rsp_tx })
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
        tx: mpsc::Sender<FatCrabMakerNotif>,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    UnregisterNotifTx {
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
}

struct FatCrabMakerActor {
    rx: mpsc::Receiver<FatCrabMakerRequest>,
    notif_tx: Option<mpsc::Sender<FatCrabMakerNotif>>,
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
            notif_tx: None,
            order,
            n3xb_maker,
        }
    }

    async fn run(&mut self) {
        while let Some(req) = self.rx.recv().await {
            match req {
                FatCrabMakerRequest::RegisterNotifTx { tx, rsp_tx } => {
                    self.register_notif_tx(tx, rsp_tx).await;
                }
                FatCrabMakerRequest::UnregisterNotifTx { rsp_tx } => {
                    self.unregister_notif_tx(rsp_tx).await;
                }
            }
        }
    }

    async fn register_notif_tx(
        &mut self,
        tx: mpsc::Sender<FatCrabMakerNotif>,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    ) {
        let mut result = Ok(());
        if self.notif_tx.is_some() {
            let error = FatCrabError::Simple {
                description: format!(
                    "Maker w/ TradeUUID {} already have notif_tx registered",
                    self.order.trade_uuid()
                ),
            };
            result = Err(error);
        }
        self.notif_tx = Some(tx);
        rsp_tx.send(result).unwrap();
    }

    async fn unregister_notif_tx(&mut self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        let mut result = Ok(());
        if self.notif_tx.is_none() {
            let error = FatCrabError::Simple {
                description: format!(
                    "Maker w/ TradeUUID {} expected to already have notif_tx registered",
                    self.order.trade_uuid()
                ),
            };
            result = Err(error);
        }
        self.notif_tx = None;
        rsp_tx.send(result).unwrap();
    }
}
