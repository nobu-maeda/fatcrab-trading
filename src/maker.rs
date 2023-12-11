use std::marker::PhantomData;
use std::str::FromStr;

use bitcoin::{Address, Txid};
use crusty_n3xb::common::error::N3xbError;
use crusty_n3xb::offer::OfferEnvelope;
use crusty_n3xb::peer_msg::PeerEnvelope;
use log::{error, warn};

use crusty_n3xb::machine::maker::MakerAccess;
use crusty_n3xb::trade_rsp::{TradeResponseBuilder, TradeResponseStatus};
use tokio::select;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

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

// Just for Typing the Maker purposes
pub struct MakerBuy {}
pub struct MakerSell {}

pub enum FatCrabMakerAccessEnum {
    Buy(FatCrabMakerAccess<MakerBuy>),
    Sell(FatCrabMakerAccess<MakerSell>),
}

#[derive(Clone)]
pub struct FatCrabMakerAccess<OrderType = MakerBuy> {
    tx: mpsc::Sender<FatCrabMakerRequest>,
    _order_type: PhantomData<OrderType>,
}

impl FatCrabMakerAccess<MakerBuy> {
    pub async fn release_notify_peer(&self) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::ReleaseNotifyPeer {
                fatcrab_txid: None,
                rsp_tx,
            })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }
}

impl FatCrabMakerAccess<MakerSell> {
    pub async fn check_btc_tx_confirmation(&self) -> Result<u32, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<u32, FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::CheckBtcTxConf { rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub async fn notify_peer(&self, fatcrab_txid: impl Into<String>) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::ReleaseNotifyPeer {
                fatcrab_txid: Some(fatcrab_txid.into()),
                rsp_tx,
            })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }
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

pub(crate) enum FatCrabMakerEnum {
    Buy(FatCrabMaker<MakerBuy>),
    Sell(FatCrabMaker<MakerSell>),
}

pub(crate) struct FatCrabMaker<OrderType = MakerBuy> {
    tx: mpsc::Sender<FatCrabMakerRequest>,
    task_handle: tokio::task::JoinHandle<()>,
    _order_type: PhantomData<OrderType>,
}

impl FatCrabMaker {
    const MAKE_TRADE_REQUEST_CHANNEL_SIZE: usize = 10;
}

impl FatCrabMaker<MakerBuy> {
    pub(crate) async fn new(
        order: FatCrabOrder,
        fatcrab_rx_addr: impl Into<String>,
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
    ) -> Self {
        let (tx, rx) =
            mpsc::channel::<FatCrabMakerRequest>(FatCrabMaker::MAKE_TRADE_REQUEST_CHANNEL_SIZE);
        let mut actor =
            FatCrabMakerActor::new(rx, order, Some(fatcrab_rx_addr.into()), n3xb_maker, purse)
                .await;
        let task_handle = tokio::spawn(async move { actor.run().await });
        Self {
            tx,
            task_handle,
            _order_type: PhantomData,
        }
    }

    pub(crate) fn new_accessor(&self) -> FatCrabMakerAccess<MakerBuy> {
        FatCrabMakerAccess {
            tx: self.tx.clone(),
            _order_type: PhantomData,
        }
    }
}

impl FatCrabMaker<MakerSell> {
    pub(crate) async fn new(
        order: FatCrabOrder,
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
    ) -> Self {
        let (tx, rx) =
            mpsc::channel::<FatCrabMakerRequest>(FatCrabMaker::MAKE_TRADE_REQUEST_CHANNEL_SIZE);
        let mut actor = FatCrabMakerActor::new(rx, order, None, n3xb_maker, purse).await;
        let task_handle = tokio::spawn(async move { actor.run().await });
        Self {
            tx,
            task_handle,
            _order_type: PhantomData,
        }
    }

    pub(crate) fn new_accessor(&self) -> FatCrabMakerAccess<MakerSell> {
        FatCrabMakerAccess {
            tx: self.tx.clone(),
            _order_type: PhantomData,
        }
    }
}

enum FatCrabMakerRequest {
    TradeResponse {
        trade_rsp_type: FatCrabTradeRspType,
        offer_envelope: FatCrabOfferEnvelope,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    ReleaseNotifyPeer {
        fatcrab_txid: Option<String>,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    CheckBtcTxConf {
        rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>,
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
    inner: FatCrabMakerInnerActor,
    rx: mpsc::Receiver<FatCrabMakerRequest>,
    order: FatCrabOrder,
    notif_tx: Option<mpsc::Sender<FatCrabMakerNotif>>,
    n3xb_maker: MakerAccess,
}

impl FatCrabMakerActor {
    async fn new(
        rx: mpsc::Receiver<FatCrabMakerRequest>,
        order: FatCrabOrder,
        fatcrab_rx_addr: Option<String>,
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
    ) -> Self {
        n3xb_maker.post_new_order().await.unwrap();

        let inner = match order.order_type {
            FatCrabOrderType::Buy => {
                let buy_actor = FatCrabMakerBuyActor::new(
                    order.clone(),
                    fatcrab_rx_addr.unwrap(),
                    n3xb_maker.clone(),
                    purse,
                )
                .await;
                FatCrabMakerInnerActor::Buy(buy_actor)
            }

            FatCrabOrderType::Sell => {
                let sell_actor =
                    FatCrabMakerSellActor::new(order.clone(), n3xb_maker.clone(), purse).await;
                FatCrabMakerInnerActor::Sell(sell_actor)
            }
        };

        Self {
            inner,
            rx,
            order,
            notif_tx: None,
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
                        FatCrabMakerRequest::TradeResponse { trade_rsp_type, offer_envelope, rsp_tx } => {
                            match self.inner {
                                FatCrabMakerInnerActor::Buy(ref mut buy_actor) => {
                                    buy_actor.trade_response(trade_rsp_type, offer_envelope, rsp_tx).await;
                                },
                                FatCrabMakerInnerActor::Sell(ref mut sell_actor) => {
                                    sell_actor.trade_response(trade_rsp_type, offer_envelope, rsp_tx).await;
                                }
                            }
                        },
                        FatCrabMakerRequest::ReleaseNotifyPeer { fatcrab_txid, rsp_tx } => {
                            match self.inner {
                                FatCrabMakerInnerActor::Buy(ref mut buy_actor) => {
                                    buy_actor.release_notify_peer(rsp_tx).await;
                                },
                                FatCrabMakerInnerActor::Sell(ref mut sell_actor) => {
                                    sell_actor.notify_peer(fatcrab_txid.unwrap(), rsp_tx).await;
                                }
                            }
                        },
                        FatCrabMakerRequest::CheckBtcTxConf { rsp_tx } => {
                            match self.inner {
                                FatCrabMakerInnerActor::Buy(ref mut buy_actor) => {
                                    buy_actor.check_btc_tx_confirmation(rsp_tx).await;
                                },
                                FatCrabMakerInnerActor::Sell(ref mut sell_actor) => {
                                    sell_actor.check_btc_tx_confirmation(rsp_tx).await;
                                }
                            }
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
                    self.handle_offer_notif(offer_result).await;
                },

                Some(peer_result) = peer_notif_rx.recv() => {
                    self.handle_peer_notif(peer_result).await;
                },
            }
        }
    }

    fn set_notif_tx(&mut self, tx: Option<mpsc::Sender<FatCrabMakerNotif>>) {
        self.notif_tx = tx.clone();

        match self.inner {
            FatCrabMakerInnerActor::Buy(ref mut buy_actor) => {
                buy_actor.notif_tx = tx;
            }
            FatCrabMakerInnerActor::Sell(ref mut sell_actor) => {
                sell_actor.notif_tx = tx;
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
        self.set_notif_tx(Some(tx));
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
        self.set_notif_tx(None);
        rsp_tx.send(result).unwrap();
    }

    async fn handle_offer_notif(&mut self, offer_result: Result<OfferEnvelope, N3xbError>) {
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
                            notif_tx
                                .send(FatCrabMakerNotif::Offer(fatcrab_offer_envelope))
                                .await
                                .unwrap();
                        } else {
                            warn!(
                                "Maker w/ TradeUUID {} do not have notif_tx registered",
                                trade_uuid.to_string()
                            );
                        }
                    }
                    Err(error) => {
                        error!(
                            "Maker w/ TradeUUID {} Offer Validation Error - {}",
                            trade_uuid.to_string(),
                            error.to_string()
                        );
                    }
                }
            }
            Err(error) => {
                error!(
                    "Maker w/ TradeUUID {} Offer Notification Rx Error - {}",
                    trade_uuid.to_string(),
                    error.to_string()
                );
            }
        }
    }

    async fn handle_peer_notif(&mut self, peer_result: Result<PeerEnvelope, N3xbError>) {
        match peer_result {
            Ok(n3xb_peer_envelope) => {
                let fatcrab_peer_message = n3xb_peer_envelope
                    .message
                    .downcast_ref::<FatCrabPeerMessage>()
                    .unwrap()
                    .clone();

                match self.inner {
                    FatCrabMakerInnerActor::Buy(ref mut buy_actor) => {
                        if buy_actor.handle_peer_notif(&fatcrab_peer_message).await {
                            return;
                        }
                    }
                    FatCrabMakerInnerActor::Sell(ref mut sell_actor) => {
                        if sell_actor.handle_peer_notif(&fatcrab_peer_message).await {
                            return;
                        }
                    }
                }

                if let Some(notif_tx) = &self.notif_tx {
                    let fatcrab_peer_envelope = FatCrabPeerEnvelope {
                        envelope: n3xb_peer_envelope,
                        message: fatcrab_peer_message,
                    };

                    notif_tx
                        .send(FatCrabMakerNotif::Peer(fatcrab_peer_envelope))
                        .await
                        .unwrap();
                } else {
                    warn!(
                        "Maker w/ TradeUUID {} do not have notif_tx registered",
                        self.order.trade_uuid
                    );
                }
            }
            Err(error) => {
                error!("Peer Notification Rx Error - {}", error.to_string());
            }
        }
    }
}

enum FatCrabMakerInnerActor {
    Buy(FatCrabMakerBuyActor),
    Sell(FatCrabMakerSellActor),
}

struct FatCrabMakerBuyActor {
    notif_tx: Option<mpsc::Sender<FatCrabMakerNotif>>,
    order: FatCrabOrder,
    fatcrab_rx_addr: String,
    btc_funds_id: Uuid,
    peer_btc_addr: Option<Address>,
    n3xb_maker: MakerAccess,
    purse: PurseAccess,
}

impl FatCrabMakerBuyActor {
    async fn new(
        order: FatCrabOrder,
        fatcrab_rx_addr: String,
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
    ) -> Self {
        let sats = order.amount * order.price;
        let btc_funds_id = purse.allocate_funds(sats as u64).await.unwrap();

        Self {
            notif_tx: None,
            order,
            fatcrab_rx_addr,
            btc_funds_id,
            peer_btc_addr: None,
            n3xb_maker,
            purse,
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
                    receive_address: self.fatcrab_rx_addr.clone(),
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

    async fn handle_peer_notif(&mut self, fatcrab_peer_message: &FatCrabPeerMessage) -> bool {
        // Should be recieving Fatcrab Tx ID along with BTC Rx Address here
        // Retain the BTC Rx Address, and notify User to confirm Fatcrab remittance
        let address = Address::from_str(&fatcrab_peer_message.receive_address).unwrap();
        let btc_addr = match address.require_network(self.purse.network) {
            Ok(address) => address,
            Err(error) => {
                error!(
                    "Maker w/ TradeUUID {} Address received from Peer {:?} is not {}",
                    self.order.trade_uuid, fatcrab_peer_message.receive_address, self.purse.network,
                );
                return true;
            }
        };
        self.peer_btc_addr = Some(btc_addr);
        return false;
    }

    async fn check_btc_tx_confirmation(&self, rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>) {
        rsp_tx
            .send(Err(FatCrabError::Simple {
                description: format!(
                    "Maker w/ TradeUUID {} is a Buyer, will not have a Peer BTC Txid to check on",
                    self.order.trade_uuid
                ),
            }))
            .unwrap();
    }

    async fn release_notify_peer(&self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        let btc_addr = match self.peer_btc_addr.clone() {
            Some(peer_btc_addr) => peer_btc_addr,
            None => {
                let error = FatCrabError::Simple {
                    description: format!(
                        "Maker w/ TradeUUID {} should have received BTC address from peer",
                        self.order.trade_uuid
                    ),
                };
                rsp_tx.send(Err(error)).unwrap();
                return;
            }
        };

        let txid = match self.purse.send_funds(self.btc_funds_id, btc_addr).await {
            Ok(txid) => txid,
            Err(error) => {
                rsp_tx.send(Err(error.into())).unwrap();
                return;
            }
        };

        let message = FatCrabPeerMessage {
            receive_address: self.fatcrab_rx_addr.clone(),
            txid: txid.to_string(),
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
}

struct FatCrabMakerSellActor {
    notif_tx: Option<mpsc::Sender<FatCrabMakerNotif>>,
    order: FatCrabOrder,
    btc_rx_addr: Address,
    peer_btc_txid: Option<Txid>,
    n3xb_maker: MakerAccess,
    purse: PurseAccess,
}

impl FatCrabMakerSellActor {
    pub async fn new(order: FatCrabOrder, n3xb_maker: MakerAccess, purse: PurseAccess) -> Self {
        let btc_rx_addr = purse.get_rx_address().await.unwrap();

        Self {
            notif_tx: None,
            order,
            btc_rx_addr,
            peer_btc_txid: None,
            n3xb_maker,
            purse,
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
                    receive_address: self.btc_rx_addr.to_string().clone(),
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

    async fn handle_peer_notif(&mut self, fatcrab_peer_message: &FatCrabPeerMessage) -> bool {
        // Should be recieving BTC Txid along with Fatcrab Rx Address here
        // Retain and Notify User
        let txid = match Txid::from_str(&fatcrab_peer_message.txid) {
            Ok(txid) => txid,
            Err(error) => {
                error!(
                    "Maker w/ TradeUUID {} Txid received from Peer {:?} is not valid",
                    self.order.trade_uuid, fatcrab_peer_message.txid,
                );
                return true;
            }
        };

        self.peer_btc_txid = Some(txid);
        return false;
    }

    async fn check_btc_tx_confirmation(&self, rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>) {
        let txid = match self.peer_btc_txid {
            Some(txid) => txid,
            None => {
                let error = FatCrabError::Simple {
                    description: format!(
                        "Maker w/ TradeUUID {} should have received BTC Txid from peer",
                        self.order.trade_uuid
                    ),
                };
                rsp_tx.send(Err(error)).unwrap();
                return;
            }
        };

        let tx_conf = match self.purse.get_tx_conf(txid).await {
            Ok(tx_conf) => tx_conf,
            Err(error) => {
                rsp_tx.send(Err(error.into())).unwrap();
                return;
            }
        };

        rsp_tx.send(Ok(tx_conf)).unwrap();
    }

    async fn notify_peer(&self, txid: String, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        let message = FatCrabPeerMessage {
            receive_address: self.btc_rx_addr.to_string().clone(),
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
}
