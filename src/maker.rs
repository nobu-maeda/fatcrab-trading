use log::{error, warn};

use crusty_n3xb::machine::maker::MakerAccess;
use crusty_n3xb::trade_rsp::{TradeResponseBuilder, TradeResponseStatus};
use tokio::select;
use tokio::sync::{mpsc, oneshot};

use crate::error::FatCrabError;
use crate::offer::{FatCrabOffer, FatCrabOfferEnvelope};
use crate::order::{FatCrabOrder, FatCrabOrderType};
use crate::peer::{FatCrabPeerEnvelope, FatCrabPeerMessage};
use crate::purse::PurseAccess;
use crate::trade_rsp::{FatCrabMakeTradeRspSpecifics, FatCrabTradeRspType};

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
        trade_rsp_type: FatCrabTradeRspType,
        offer_envelope: FatCrabOfferEnvelope,
    ) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::TradeResponse {
                trade_rsp_type,
                offer_envelope,
                rsp_tx,
            })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub async fn notify_peer(&self, txid: impl Into<String>) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::NotifyPeer {
                txid: txid.into(),
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

    pub(crate) async fn new(
        order: FatCrabOrder,
        fatcrab_rx_address: Option<String>,
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<FatCrabMakerRequest>(Self::MAKE_TRADE_REQUEST_CHANNEL_SIZE);
        let mut actor =
            FatCrabMakerActor::new(rx, order, fatcrab_rx_address, n3xb_maker, purse).await;
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
        trade_rsp_type: FatCrabTradeRspType,
        offer_envelope: FatCrabOfferEnvelope,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    NotifyPeer {
        txid: String,
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
    receive_address: String,
    n3xb_maker: MakerAccess,
    purse: PurseAccess,
}

impl FatCrabMakerActor {
    async fn new(
        rx: mpsc::Receiver<FatCrabMakerRequest>,
        order: FatCrabOrder,
        fatcrab_rx_address: Option<String>,
        n3xb_maker: MakerAccess,
        purse: PurseAccess, // TODO: Gotta support something aside from Memory Database down the road
    ) -> Self {
        n3xb_maker.post_new_order().await.unwrap();

        // For Buy Order, Maker receive_address is Fatcrab ID
        let receive_address = match order.order_type {
            FatCrabOrderType::Buy => {
                let receive_address =
                    fatcrab_rx_address.expect("Fatcrab ID is required for Buy Order");
                if let Some(error) = purse.allocate_funds(order.amount as u64).await.err() {
                    error!("Cannot allocate funds from Wallet - {}", error.to_string());
                }
                receive_address
            }
            FatCrabOrderType::Sell => purse
                .get_rx_address()
                .await
                .expect("Cannot create receive address from Wallet")
                .to_string(),
        };

        Self {
            rx,
            notif_tx: None,
            order,
            receive_address,
            n3xb_maker,
            purse,
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
                        FatCrabMakerRequest::TradeResponse { trade_rsp_type, offer_envelope, rsp_tx } => {
                            self.trade_response(trade_rsp_type, offer_envelope, rsp_tx).await;
                        },
                        FatCrabMakerRequest::NotifyPeer { txid, rsp_tx } => {
                            self.notify_peer(txid, rsp_tx).await;
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
                    let trade_uuid = self.order.trade_uuid;

                    match offer_result {
                        Ok(n3xb_offer_envelope) => {
                            let n3xb_offer = n3xb_offer_envelope.offer.clone();
                            match FatCrabOffer::validate_n3xb_offer(n3xb_offer) {
                                Ok(_) => {
                                    if let Some(notif_tx) = &self.notif_tx {
                                        let fatcrab_offer_envelope = FatCrabOfferEnvelope {
                                            envelope: n3xb_offer_envelope,
                                        };
                                        notif_tx.send(FatCrabMakerNotif::Offer(fatcrab_offer_envelope)).await.unwrap();
                                    } else {
                                        warn!("Maker w/ TradeUUID {} do not have notif_tx registered", trade_uuid.to_string());
                                    }
                                },
                                Err(error) => {
                                    error!("Maker w/ TradeUUID {} Offer Validation Error - {}", trade_uuid.to_string(), error.to_string());
                                    continue;
                                }
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
                            let fatcrab_peer_message = n3xb_peer_envelope.message.downcast_ref::<FatCrabPeerMessage>().unwrap().clone();

                            match self.order.order_type {
                                FatCrabOrderType::Buy => {
                                    // User to confirm Fatcrab remittance, before releasing Bitcoin & nofity Peer
                                },
                                FatCrabOrderType::Sell => {
                                    // Log Bitcoin Txid, notify User so User can check on TxID
                                }
                            }

                            if let Some(notif_tx) = &self.notif_tx {
                                let fatcrab_peer_envelope = FatCrabPeerEnvelope {
                                    envelope: n3xb_peer_envelope,
                                    message: fatcrab_peer_message
                                };

                                notif_tx.send(FatCrabMakerNotif::Peer(fatcrab_peer_envelope)).await.unwrap();
                            } else {
                                warn!("Maker w/ TradeUUID {} do not have notif_tx registered", self.order.trade_uuid);
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
        trade_rsp_type: FatCrabTradeRspType,
        offer_envelope: FatCrabOfferEnvelope,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    ) {
        match trade_rsp_type {
            FatCrabTradeRspType::Accept => {
                let mut trade_rsp_builder = TradeResponseBuilder::new();
                trade_rsp_builder.offer_event_id(offer_envelope.envelope.event_id);
                trade_rsp_builder.trade_response(TradeResponseStatus::Accepted);

                let trade_engine_specifics = FatCrabMakeTradeRspSpecifics {
                    receive_address: self.receive_address.clone(),
                };
                trade_rsp_builder.trade_engine_specifics(Box::new(trade_engine_specifics));

                let n3xb_trade_rsp = trade_rsp_builder.build().unwrap();
                self.n3xb_maker.accept_offer(n3xb_trade_rsp).await.unwrap();
            }
            FatCrabTradeRspType::Reject => {
                self.n3xb_maker.cancel_order().await.unwrap();
            }
        }
        rsp_tx.send(Ok(())).unwrap();
    }

    async fn notify_peer(&self, txid: String, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        let message = FatCrabPeerMessage {
            receive_address: self.receive_address.clone(),
            txid,
        };
        match self.n3xb_maker.send_peer_message(Box::new(message)).await {
            Ok(_) => {
                rsp_tx.send(Ok(())).unwrap();
            }
            Err(error) => {
                rsp_tx.send(Err(error.into())).unwrap();
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
                    self.order.trade_uuid
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
                    self.order.trade_uuid
                ),
            };
            result = Err(error);
        }
        self.notif_tx = None;
        rsp_tx.send(result).unwrap();
    }
}
