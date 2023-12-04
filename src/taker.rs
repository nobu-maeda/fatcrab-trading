use crusty_n3xb::machine::taker::TakerAccess;
use log::{error, warn};
use tokio::{
    select,
    sync::{mpsc, oneshot},
};

use crate::{
    error::FatCrabError,
    offer::FatCrabOffer,
    order::FatCrabOrderEnvelope,
    peer_msg::{FatCrabPeerEnvelope, FatCrabPeerMessage},
    trade_rsp::{FatCrabTradeRsp, FatCrabTradeRspEnvelope},
};

pub enum FatCrabTakerNotif {
    TradeResponse {
        trade_rsp_envelope: FatCrabTradeRspEnvelope,
    },
    PeerMessage {
        peer_msg_envelope: FatCrabPeerEnvelope,
    },
}

#[derive(Clone)]
pub struct FatCrabTakerAccess {
    tx: mpsc::Sender<FatCrabTakerRequest>,
}

impl FatCrabTakerAccess {
    pub async fn register_notif_tx(
        &self,
        tx: mpsc::Sender<FatCrabTakerNotif>,
    ) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::RegisterNotifTx { tx, rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub async fn unregister_notif_tx(&self) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::UnregisterNotifTx { rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }
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
        let (trade_rsp_notif_tx, mut trade_rsp_notif_rx) = mpsc::channel(5);
        let (peer_notif_tx, mut peer_notif_rx) = mpsc::channel(5);

        self.n3xb_taker
            .register_trade_notif_tx(trade_rsp_notif_tx)
            .await
            .unwrap();
        self.n3xb_taker
            .register_peer_notif_tx(peer_notif_tx)
            .await
            .unwrap();

        loop {
            select! {
                Some(request) = self.rx.recv() => {
                    match request {
                        FatCrabTakerRequest::RegisterNotifTx { tx, rsp_tx } => {
                            self.register_notif_tx(tx, rsp_tx).await;
                        }
                        FatCrabTakerRequest::UnregisterNotifTx { rsp_tx } => {
                            self.unregister_notif_tx(rsp_tx).await;
                        }
                    }
                },

                Some(trade_rsp_result) = trade_rsp_notif_rx.recv() => {
                    let trade_uuid = self.order.order.trade_uuid;

                    match trade_rsp_result {
                        Ok(n3xb_trade_rsp_envelope) => {
                            if let Some(notif_tx) = &self.notif_tx {
                                let fatcrab_trade_rsp_envelope = FatCrabTradeRspEnvelope {
                                    envelope: n3xb_trade_rsp_envelope.clone(),
                                    trade_rsp: FatCrabTradeRsp::from_n3xb_trade_rsp( n3xb_trade_rsp_envelope.trade_rsp)
                                };
                                notif_tx.send(FatCrabTakerNotif::TradeResponse { trade_rsp_envelope: fatcrab_trade_rsp_envelope }).await.unwrap();
                            } else {
                                warn!("Taker w/ TradeUUID {} do not have notif_tx registered", trade_uuid.to_string());
                            }
                        }
                        Err(error) => {
                            error!("Taker w/ TradeUUID {} Offer Notification Rx Error - {}", trade_uuid.to_string(), error.to_string());
                        }
                    }
                },

                Some(peer_result) = peer_notif_rx.recv() => {
                    let trade_uuid = self.order.order.trade_uuid;

                    match peer_result {
                        Ok(n3xb_peer_envelope) => {
                            let fatcrab_peer_msg = n3xb_peer_envelope.message.downcast_ref::<FatCrabPeerMessage>().unwrap().clone();
                            if let Some(notif_tx) = &self.notif_tx {
                                let fatcrab_peer_envelope = FatCrabPeerEnvelope {
                                    envelope: n3xb_peer_envelope,
                                    peer_msg: fatcrab_peer_msg
                                };
                                notif_tx.send(FatCrabTakerNotif::PeerMessage { peer_msg_envelope: fatcrab_peer_envelope }).await.unwrap();
                            } else {
                                warn!("Taker w/ TradeUUID {} do not have notif_tx registered", trade_uuid.to_string());
                            }
                        },
                        Err(error) => {
                            error!("Taker w/ TradeUUID {} Peer Notification Rx Error - {}", trade_uuid.to_string(), error.to_string());
                        }
                    }
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
                    self.order.order.trade_uuid
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
                    self.order.order.trade_uuid
                ),
            };
            result = Err(error);
        }
        self.notif_tx = None;
        rsp_tx.send(result).unwrap();
    }
}
