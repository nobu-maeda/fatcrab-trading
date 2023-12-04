use log::{error, warn};

use crusty_n3xb::machine::maker::MakerAccess;
use crusty_n3xb::trade_rsp::{TradeResponseBuilder, TradeResponseStatus};
use tokio::select;
use tokio::sync::{mpsc, oneshot};

use crate::error::FatCrabError;
use crate::offer::{FatCrabOffer, FatCrabOfferEnvelope};
use crate::order::FatCrabOrder;
use crate::peer_msg::{FatCrabPeerEnvelope, FatCrabPeerMsg};
use crate::trade_rsp::{FatCrabMakeTradeRspSpecifics, FatCrabTradeRsp};

pub enum FatCrabMakerNotif {
    Offer(FatCrabOfferEnvelope),
    Peer(FatCrabPeerEnvelope),
}

#[derive(Clone)]
pub struct FatCrabMakerAccess {
    tx: mpsc::Sender<FatCrabMakerRequest>,
}

impl FatCrabMakerAccess {
    pub async fn trade_response(
        &self,
        trade_rsp: FatCrabTradeRsp,
        offer_envelope: FatCrabOfferEnvelope,
    ) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::TradeResponse {
                trade_rsp,
                offer_envelope,
                rsp_tx,
            })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

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
    TradeResponse {
        trade_rsp: FatCrabTradeRsp,
        offer_envelope: FatCrabOfferEnvelope,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
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
        let (offer_notif_tx, mut offer_notif_rx) = mpsc::channel(5);
        let (peer_notif_tx, mut peer_notif_rx) = mpsc::channel(5);

        self.n3xb_maker
            .register_offer_notif_tx(offer_notif_tx)
            .await
            .unwrap();

        self.n3xb_maker
            .register_peer_notif_tx(peer_notif_tx)
            .await
            .unwrap();

        loop {
            select! {
                Some(request) = self.rx.recv() => {
                    match request {
                        FatCrabMakerRequest::TradeResponse { trade_rsp, offer_envelope, rsp_tx } => {
                            self.trade_response(trade_rsp, offer_envelope, rsp_tx).await;
                        },
                        FatCrabMakerRequest::RegisterNotifTx { tx, rsp_tx } => {
                            self.register_notif_tx(tx, rsp_tx).await;
                        },
                        FatCrabMakerRequest::UnregisterNotifTx { rsp_tx } => {
                            self.unregister_notif_tx(rsp_tx).await;
                        },
                    }
                },

                Some(offer_result) = offer_notif_rx.recv() => {
                    let trade_uuid = self.order.trade_uuid();

                    match offer_result {
                        Ok(n3xb_offer_envelope) => {
                            if let Some(notif_tx) = &self.notif_tx {
                                let n3xb_offer = n3xb_offer_envelope.offer.clone();
                                let fatcrab_offer_envelope = FatCrabOfferEnvelope {
                                    envelope: n3xb_offer_envelope,
                                    offer: FatCrabOffer::from_n3xb_offer(n3xb_offer).unwrap(),
                                };
                                notif_tx.send(FatCrabMakerNotif::Offer(fatcrab_offer_envelope)).await.unwrap();
                            } else {
                                warn!("Maker w/ TradeUUID {} do not have notif_tx registered", trade_uuid.to_string());
                            }
                        },
                        Err(error) => {
                            error!("Maker w/ TradeUUID {} Offer Notification Rx Error - {}", trade_uuid.to_string(), error.to_string());
                        }
                    }

                },

                Some(peer_result) = peer_notif_rx.recv() => {
                    match peer_result {
                        Ok(n3xb_peer_envelope) => {
                            if let Some(notif_tx) = &self.notif_tx {
                                let fatcrab_peer_envelope = FatCrabPeerEnvelope {
                                    envelope: n3xb_peer_envelope,
                                    peer_msg: FatCrabPeerMsg {}
                                };
                                notif_tx.send(FatCrabMakerNotif::Peer(fatcrab_peer_envelope)).await.unwrap();
                            } else {
                                warn!("Maker w/ TradeUUID {} do not have notif_tx registered", self.order.trade_uuid());
                            }
                        },
                        Err(error) => {
                            error!("Peer Notification Rx Error - {}", error.to_string());
                        }
                    }
                },
            }
        }
    }

    async fn trade_response(
        &mut self,
        trade_rsp: FatCrabTradeRsp,
        offer_envelope: FatCrabOfferEnvelope,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    ) {
        match trade_rsp {
            FatCrabTradeRsp::Accept => {
                let mut trade_rsp_builder = TradeResponseBuilder::new();
                trade_rsp_builder.offer_event_id(offer_envelope.envelope.event_id);
                trade_rsp_builder.trade_response(TradeResponseStatus::Accepted);

                let trade_engine_specifics = FatCrabMakeTradeRspSpecifics {};
                trade_rsp_builder.trade_engine_specifics(Box::new(trade_engine_specifics));

                let n3xb_trade_rsp = trade_rsp_builder.build().unwrap();
                self.n3xb_maker.accept_offer(n3xb_trade_rsp).await.unwrap();
            }
            FatCrabTradeRsp::Reject => {
                self.n3xb_maker.cancel_order().await.unwrap();
            }
        }
        rsp_tx.send(Ok(())).unwrap();
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
