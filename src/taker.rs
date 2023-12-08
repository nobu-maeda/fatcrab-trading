use log::{error, warn};

use bitcoin::{address::Address, address::NetworkChecked, address::NetworkUnchecked};
use crusty_n3xb::machine::taker::TakerAccess;
use tokio::{
    select,
    sync::{mpsc, oneshot},
};

use crate::{
    error::FatCrabError,
    order::{FatCrabOrderEnvelope, FatCrabOrderType},
    peer::{FatCrabPeerEnvelope, FatCrabPeerMessage},
    purse::PurseAccess,
    trade_rsp::{FatCrabMakeTradeRspSpecifics, FatCrabTradeRsp, FatCrabTradeRspEnvelope},
};

pub enum FatCrabTakerNotif {
    TradeRsp(FatCrabTradeRspEnvelope),
    Peer(FatCrabPeerEnvelope),
}

#[derive(Clone)]
pub struct FatCrabTakerAccess {
    tx: mpsc::Sender<FatCrabTakerRequest>,
}

impl FatCrabTakerAccess {
    pub async fn notify_peer(&self, txid: impl Into<String>) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::NotifyPeer {
                txid: txid.into(),
                rsp_tx,
            })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

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
        order: FatCrabOrderEnvelope,
        fatcrab_rx_address: Option<String>,
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<FatCrabTakerRequest>(Self::TAKE_TRADE_REQUEST_CHANNEL_SIZE);
        let mut actor =
            FatCrabTakerActor::new(rx, order, fatcrab_rx_address, n3xb_taker, purse).await;
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
    NotifyPeer {
        txid: String,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
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
    order: FatCrabOrderEnvelope,
    receive_address: String,
    allocation: u64,
    n3xb_taker: TakerAccess,
    purse: PurseAccess,
}

impl FatCrabTakerActor {
    async fn new(
        rx: mpsc::Receiver<FatCrabTakerRequest>,
        order: FatCrabOrderEnvelope,
        fatcrab_rx_address: Option<String>,
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
    ) -> Self {
        let mut allocation = 0;

        // For Buy Order, Taker receive_address is Bitcoin address
        // For Buy Order, Maker receive_address is Fatcrab ID
        let receive_address = match order.order.order_type {
            FatCrabOrderType::Buy => purse
                .get_rx_address()
                .await
                .expect("Cannot create receive address from Wallet")
                .to_string(),

            FatCrabOrderType::Sell => {
                let receive_address =
                    fatcrab_rx_address.expect("Fatcrab ID is required to Take a Buy Order");
                if let Some(error) = purse.allocate_funds(order.order.amount as u64).await.err() {
                    error!("Cannot allocate funds from Wallet - {}", error.to_string());
                }
                allocation = order.order.amount as u64;
                receive_address
            }
        };

        Self {
            rx,
            notif_tx: None,
            order,
            receive_address,
            allocation,
            n3xb_taker,
            purse,
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
                        FatCrabTakerRequest::NotifyPeer { txid, rsp_tx } => {
                            self.notify_peer(txid, rsp_tx).await;
                        }
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
                            let trade_rsp = FatCrabTradeRsp::from_n3xb_trade_rsp(n3xb_trade_rsp_envelope.trade_rsp.clone());

                            match trade_rsp.clone() {
                                FatCrabTradeRsp::Accept(receive_address) => {
                                    if FatCrabOrderType::Sell == self.order.order.order_type {
                                        // For Maker Sell, Taker Buy Order
                                        // There's nothing preventing auto remit pre-allocated funds to Maker
                                        // Delay notifying User. User will be notified when Maker remits Fatcrab peer notificaiton
                                        let btc_unchecked_address: Address<NetworkUnchecked> = (&receive_address).parse().unwrap();
                                        let btc_receive_address = btc_unchecked_address.require_network(self.purse.network).unwrap();

                                        let txid = self.purse.send_to_address(
                                            btc_receive_address,
                                            self.order.order.amount as u64
                                        ).await.unwrap();

                                        let message = FatCrabPeerMessage {
                                            receive_address: self.receive_address.clone(),
                                            txid: txid.to_string(),
                                        };
                                        self.n3xb_taker.send_peer_message(Box::new(message)).await;
                                        continue;
                                    }
                                },
                                FatCrabTradeRsp::Reject => {},
                            }

                            if let Some(notif_tx) = &self.notif_tx {
                                // Notify User to remite Fatcrab to Maker
                                let fatcrab_trade_rsp_envelope = FatCrabTradeRspEnvelope {
                                    envelope: n3xb_trade_rsp_envelope.clone(),
                                    trade_rsp
                                };
                                notif_tx.send(FatCrabTakerNotif::TradeRsp(fatcrab_trade_rsp_envelope)).await.unwrap();
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
                            let fatcrab_peer_message = n3xb_peer_envelope.message.downcast_ref::<FatCrabPeerMessage>().unwrap().clone();
                            if let Some(notif_tx) = &self.notif_tx {
                                let fatcrab_peer_envelope = FatCrabPeerEnvelope {
                                    envelope: n3xb_peer_envelope,
                                    message: fatcrab_peer_message
                                };
                                notif_tx.send(FatCrabTakerNotif::Peer(fatcrab_peer_envelope)).await.unwrap();
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

    async fn notify_peer(
        &mut self,
        txid: String,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    ) {
        let message = FatCrabPeerMessage {
            receive_address: self.receive_address.clone(),
            txid,
        };

        match self.n3xb_taker.send_peer_message(Box::new(message)).await {
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
