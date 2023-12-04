use crusty_n3xb::machine::taker::TakerAccess;
use tokio::sync::{mpsc, oneshot};

use crate::{error::FatCrabError, offer::FatCrabOffer, order::FatCrabOrderEnvelope};

pub enum FatCrabTakerNotif {
    TradeRsp,
}

#[derive(Clone)]
pub struct FatCrabTakerAccess {
    tx: mpsc::Sender<FatCrabTakerRequest>,
}

pub(crate) struct FatCrabTaker {
    tx: mpsc::Sender<FatCrabTakerRequest>,
    task_handle: tokio::task::JoinHandle<()>,
}

impl FatCrabTaker {
    const TAKE_TRADE_REQUEST_CHANNEL_SIZE: usize = 10;

    pub(crate) async fn new(
        offer: FatCrabOffer,
        order: FatCrabOrderEnvelope,
        n3xb_taker: TakerAccess,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<FatCrabTakerRequest>(Self::TAKE_TRADE_REQUEST_CHANNEL_SIZE);
        let mut actor = FatCrabTakerActor::new(rx, offer, order, n3xb_taker).await;
        let task_handle = tokio::spawn(async move { actor.run().await });
        Self { tx, task_handle }
    }

    pub(crate) fn new_accessor(&self) -> FatCrabTakerAccess {
        FatCrabTakerAccess {
            tx: self.tx.clone(),
        }
    }
}

enum FatCrabTakerRequest {
    RegisterNotifTx {
        tx: mpsc::Sender<FatCrabTakerNotif>,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    UnregisterNotifTx {
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
}

struct FatCrabTakerActor {
    rx: mpsc::Receiver<FatCrabTakerRequest>,
    notif_tx: Option<mpsc::Sender<FatCrabTakerNotif>>,
    offer: FatCrabOffer,
    order: FatCrabOrderEnvelope,
    n3xb_taker: TakerAccess,
}

impl FatCrabTakerActor {
    async fn new(
        rx: mpsc::Receiver<FatCrabTakerRequest>,
        offer: FatCrabOffer,
        order: FatCrabOrderEnvelope,
        n3xb_taker: TakerAccess,
    ) -> Self {
        Self {
            rx,
            notif_tx: None,
            offer,
            order,
            n3xb_taker,
        }
    }

    async fn run(&mut self) {
        while let Some(req) = self.rx.recv().await {
            match req {
                FatCrabTakerRequest::RegisterNotifTx { tx, rsp_tx } => {
                    self.register_notif_tx(tx, rsp_tx).await;
                }
                FatCrabTakerRequest::UnregisterNotifTx { rsp_tx } => {
                    self.unregister_notif_tx(rsp_tx).await;
                }
            }
        }
    }

    async fn register_notif_tx(
        &mut self,
        tx: mpsc::Sender<FatCrabTakerNotif>,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    ) {
        let mut result = Ok(());
        if self.notif_tx.is_some() {
            let error = FatCrabError::Simple {
                description: format!(
                    "Taker w/ TradeUUID {} already have notif_tx registered",
                    self.order.order.trade_uuid()
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
                    "Taker w/ TradeUUID {} expected to already have notif_tx registered",
                    self.order.order.trade_uuid()
                ),
            };
            result = Err(error);
        }
        self.notif_tx = None;
        rsp_tx.send(result).unwrap();
    }
}
