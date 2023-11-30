use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crusty_n3xb::machine::maker::MakerAccess;

use crate::error::FatCrabError;
use crate::trade_order::TradeOrder;

#[derive(Clone)]
pub struct MakeTradeAccess {
    tx: mpsc::Sender<MakeTradeRequest>,
}

impl MakeTradeAccess {
    async fn new(tx: mpsc::Sender<MakeTradeRequest>) -> Self {
        Self { tx }
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

pub(crate) struct MakeTrade {
    tx: mpsc::Sender<MakeTradeRequest>,
    task_handle: tokio::task::JoinHandle<()>,
}

impl MakeTrade {
    const MAKE_TRADE_REQUEST_CHANNEL_SIZE: usize = 10;

    pub(crate) fn new(trade_order: TradeOrder, n3xb_maker: MakerAccess) -> Self {
        let (tx, rx) = mpsc::channel::<MakeTradeRequest>(Self::MAKE_TRADE_REQUEST_CHANNEL_SIZE);
        let mut actor = MakeTradeActor::new(rx, trade_order, n3xb_maker);
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
    RegisterNotifTx {
        tx: mpsc::Sender<String>,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
}

struct MakeTradeActor {
    rx: mpsc::Receiver<MakeTradeRequest>,
    trade_order: TradeOrder,
    n3xb_maker: MakerAccess,
}

impl MakeTradeActor {
    fn new(
        rx: mpsc::Receiver<MakeTradeRequest>,
        trade_order: TradeOrder,
        n3xb_maker: MakerAccess,
    ) -> Self {
        Self {
            rx,
            trade_order,
            n3xb_maker,
        }
    }

    async fn run(&mut self) {
        while let Some(req) = self.rx.recv().await {
            match req {
                MakeTradeRequest::RegisterNotifTx { tx, rsp_tx } => {
                    tx.send(Uuid::new_v4().to_string()).await.unwrap();
                    rsp_tx.send(Ok(())).unwrap();
                }
            }
        }
    }
}
