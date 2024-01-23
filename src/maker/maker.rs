use std::marker::PhantomData;
use std::path::Path;
use std::str::FromStr;

use bitcoin::{Address, Txid};
use crusty_n3xb::offer::OfferEnvelope;
use crusty_n3xb::peer_msg::PeerEnvelope;
use log::{error, warn};

use crusty_n3xb::maker::{MakerAccess, MakerNotif};
use crusty_n3xb::trade_rsp::{TradeResponseBuilder, TradeResponseStatus};
use tokio::select;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinError;
use uuid::Uuid;

use crate::error::FatCrabError;
use crate::offer::{FatCrabOffer, FatCrabOfferEnvelope};
use crate::order::{FatCrabOrder, FatCrabOrderType};
use crate::peer::{FatCrabPeerEnvelope, FatCrabPeerMessage};
use crate::purse::PurseAccess;
use crate::trade_rsp::{FatCrabMakeTradeRspSpecifics, FatCrabTradeRspType};

use super::data::{FatCrabMakerBuyData, FatCrabMakerSellData};

pub enum FatCrabMakerNotif {
    Offer(FatCrabOfferEnvelope),
    Peer(FatCrabPeerEnvelope),
}

// Just for purpose of typing the Maker
#[derive(Clone)]
pub struct MakerBuy {}

#[derive(Clone)]
pub struct MakerSell {}

#[derive(Clone)]
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

impl<OrderType> FatCrabMakerAccess<OrderType> {
    pub async fn post_new_order(&self) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::PostNewOrder { rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub async fn query_offers(&self) -> Result<Vec<FatCrabOfferEnvelope>, FatCrabError> {
        let (rsp_tx, rsp_rx) =
            oneshot::channel::<Result<Vec<FatCrabOfferEnvelope>, FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::QueryOffers { rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

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

    pub async fn trade_complete(&self) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::TradeComplete { rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub async fn shutdown(&self) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::Shutdown { rsp_tx })
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

impl FatCrabMakerEnum {
    pub(crate) async fn await_task_handle(self) -> Result<(), JoinError> {
        match self {
            FatCrabMakerEnum::Buy(maker) => maker.task_handle.await,
            FatCrabMakerEnum::Sell(maker) => maker.task_handle.await,
        }
    }
}

pub(crate) struct FatCrabMaker<OrderType = MakerBuy> {
    tx: mpsc::Sender<FatCrabMakerRequest>,
    pub(crate) task_handle: tokio::task::JoinHandle<()>,
    _order_type: PhantomData<OrderType>,
}

impl FatCrabMaker {
    const MAKE_TRADE_REQUEST_CHANNEL_SIZE: usize = 10;
}

impl FatCrabMaker<MakerBuy> {
    pub(crate) async fn new(
        order: &FatCrabOrder,
        fatcrab_rx_addr: impl Into<String>,
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
        dir_path: impl AsRef<Path>,
    ) -> Self {
        assert_eq!(order.order_type, FatCrabOrderType::Buy);
        let (tx, rx) =
            mpsc::channel::<FatCrabMakerRequest>(FatCrabMaker::MAKE_TRADE_REQUEST_CHANNEL_SIZE);
        let actor = FatCrabMakerActor::new(
            rx,
            order.to_owned(),
            Some(fatcrab_rx_addr.into()),
            n3xb_maker,
            purse,
            dir_path,
        )
        .await;
        let task_handle = tokio::spawn(async move { actor.run().await });

        Self {
            tx,
            task_handle,
            _order_type: PhantomData,
        }
    }

    pub(crate) async fn restore(
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let (tx, rx) =
            mpsc::channel::<FatCrabMakerRequest>(FatCrabMaker::MAKE_TRADE_REQUEST_CHANNEL_SIZE);

        let actor = FatCrabMakerActor::restore_buy_actor(rx, n3xb_maker, purse, data_path).await?;
        let task_handle = tokio::spawn(async move { actor.run().await });

        Ok(Self {
            tx,
            task_handle,
            _order_type: PhantomData,
        })
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
        order: &FatCrabOrder,
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
        dir_path: impl AsRef<Path>,
    ) -> Self {
        assert_eq!(order.order_type, FatCrabOrderType::Sell);
        let (tx, rx) =
            mpsc::channel::<FatCrabMakerRequest>(FatCrabMaker::MAKE_TRADE_REQUEST_CHANNEL_SIZE);
        let actor =
            FatCrabMakerActor::new(rx, order.to_owned(), None, n3xb_maker, purse, dir_path).await;
        let task_handle = tokio::spawn(async move { actor.run().await });

        Self {
            tx,
            task_handle,
            _order_type: PhantomData,
        }
    }

    pub(crate) async fn restore(
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let (tx, rx) =
            mpsc::channel::<FatCrabMakerRequest>(FatCrabMaker::MAKE_TRADE_REQUEST_CHANNEL_SIZE);

        let actor = FatCrabMakerActor::restore_sell_actor(rx, n3xb_maker, purse, data_path).await?;
        let task_handle = tokio::spawn(async move { actor.run().await });

        Ok(Self {
            tx,
            task_handle,
            _order_type: PhantomData,
        })
    }

    pub(crate) fn new_accessor(&self) -> FatCrabMakerAccess<MakerSell> {
        FatCrabMakerAccess {
            tx: self.tx.clone(),
            _order_type: PhantomData,
        }
    }
}

enum FatCrabMakerRequest {
    PostNewOrder {
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    QueryOffers {
        rsp_tx: oneshot::Sender<Result<Vec<FatCrabOfferEnvelope>, FatCrabError>>,
    },
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
    TradeComplete {
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    RegisterNotifTx {
        tx: mpsc::Sender<FatCrabMakerNotif>,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    UnregisterNotifTx {
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    Shutdown {
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
}

struct FatCrabMakerActor {
    inner: FatCrabMakerInnerActor,
    trade_uuid: Uuid,
    rx: mpsc::Receiver<FatCrabMakerRequest>,
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
        dir_path: impl AsRef<Path>,
    ) -> Self {
        let inner = match order.order_type {
            FatCrabOrderType::Buy => {
                let buy_actor = FatCrabMakerBuyActor::new(
                    &order,
                    fatcrab_rx_addr.unwrap(),
                    n3xb_maker.clone(),
                    purse,
                    dir_path,
                )
                .await;
                FatCrabMakerInnerActor::Buy(buy_actor)
            }

            FatCrabOrderType::Sell => {
                let sell_actor =
                    FatCrabMakerSellActor::new(&order, n3xb_maker.clone(), purse, dir_path).await;
                FatCrabMakerInnerActor::Sell(sell_actor)
            }
        };

        Self {
            inner,
            rx,
            trade_uuid: order.trade_uuid,
            notif_tx: None,
            n3xb_maker,
        }
    }

    async fn restore_buy_actor(
        rx: mpsc::Receiver<FatCrabMakerRequest>,
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let (trade_uuid, actor) =
            FatCrabMakerBuyActor::restore(n3xb_maker.clone(), purse, data_path).await?;
        let inner = FatCrabMakerInnerActor::Buy(actor);

        Ok(Self {
            inner,
            trade_uuid,
            rx,
            notif_tx: None,
            n3xb_maker,
        })
    }

    async fn restore_sell_actor(
        rx: mpsc::Receiver<FatCrabMakerRequest>,
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let (trade_uuid, actor) =
            FatCrabMakerSellActor::restore(n3xb_maker.clone(), purse, data_path).await?;
        let inner = FatCrabMakerInnerActor::Sell(actor);

        Ok(Self {
            inner,
            trade_uuid,
            rx,
            notif_tx: None,
            n3xb_maker,
        })
    }

    async fn run(mut self) {
        let (notif_tx, mut notif_rx) = mpsc::channel(5);

        self.n3xb_maker.register_notif_tx(notif_tx).await.unwrap();

        loop {
            select! {
                Some(request) = self.rx.recv() => {
                    match request {
                        FatCrabMakerRequest::PostNewOrder { rsp_tx } => {
                            self.post_new_order(rsp_tx).await;
                        },
                        FatCrabMakerRequest::QueryOffers { rsp_tx } => {
                            self.query_offers(rsp_tx).await;
                        },
                        FatCrabMakerRequest::TradeResponse { trade_rsp_type, offer_envelope, rsp_tx } => {
                            match self.inner {
                                FatCrabMakerInnerActor::Buy(ref buy_actor) => {
                                    buy_actor.trade_response(trade_rsp_type, offer_envelope, rsp_tx).await;
                                },
                                FatCrabMakerInnerActor::Sell(ref sell_actor) => {
                                    sell_actor.trade_response(trade_rsp_type, offer_envelope, rsp_tx).await;
                                }
                            }
                        },
                        FatCrabMakerRequest::ReleaseNotifyPeer { fatcrab_txid, rsp_tx } => {
                            match self.inner {
                                FatCrabMakerInnerActor::Buy(ref buy_actor) => {
                                    buy_actor.release_notify_peer(rsp_tx).await;
                                },
                                FatCrabMakerInnerActor::Sell(ref sell_actor) => {
                                    sell_actor.notify_peer(fatcrab_txid.unwrap(), rsp_tx).await;
                                }
                            }
                        },
                        FatCrabMakerRequest::CheckBtcTxConf { rsp_tx } => {
                            match self.inner {
                                FatCrabMakerInnerActor::Buy(ref buy_actor) => {
                                    buy_actor.check_btc_tx_confirmation(rsp_tx).await;
                                },
                                FatCrabMakerInnerActor::Sell(ref sell_actor) => {
                                    sell_actor.check_btc_tx_confirmation(rsp_tx).await;
                                }
                            }
                        },
                        FatCrabMakerRequest::TradeComplete { rsp_tx } => {
                            match self.inner {
                                FatCrabMakerInnerActor::Buy(buy_actor) => {
                                    buy_actor.trade_complete(rsp_tx).await;
                                },
                                FatCrabMakerInnerActor::Sell(sell_actor) => {
                                    sell_actor.trade_complete(rsp_tx).await;
                                }
                            }
                            return;
                        },
                        FatCrabMakerRequest::Shutdown { rsp_tx } => {
                            self.shutdown(rsp_tx).await;
                            return;
                        },
                        FatCrabMakerRequest::RegisterNotifTx { tx, rsp_tx } => {
                            self.register_notif_tx(tx, rsp_tx).await;
                        },
                        FatCrabMakerRequest::UnregisterNotifTx { rsp_tx } => {
                            self.unregister_notif_tx(rsp_tx).await;
                        },
                    }
                },

                Some(result) = notif_rx.recv() => {
                    match result {
                        Ok(notif) => match notif {
                            MakerNotif::Offer(offer_envelope) => {
                                self.handle_offer_notif(offer_envelope).await;
                            },
                            MakerNotif::Peer(peer_envelope) => {
                                self.handle_peer_notif(peer_envelope).await;
                            },
                        },
                        Err(error) => {
                            error!("Maker w/ TradeUUID {} Notification Rx Error - {}", self.trade_uuid, error.to_string());
                        }
                    }
                },
            }
        }
    }

    async fn post_new_order(&mut self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        if let Some(error) = self.n3xb_maker.post_new_order().await.err() {
            rsp_tx.send(Err(error.into())).unwrap();
        } else {
            rsp_tx.send(Ok(())).unwrap();
        }
    }

    async fn query_offers(
        &self,
        rsp_tx: oneshot::Sender<Result<Vec<FatCrabOfferEnvelope>, FatCrabError>>,
    ) {
        let offer_envelopes = match self.inner {
            FatCrabMakerInnerActor::Buy(ref buy_actor) => buy_actor.data.offer_envelopes().await,
            FatCrabMakerInnerActor::Sell(ref sell_actor) => sell_actor.data.offer_envelopes().await,
        };
        rsp_tx.send(Ok(offer_envelopes)).unwrap();
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
                    self.trade_uuid
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
                    self.trade_uuid
                ),
            };
            result = Err(error);
        }
        self.set_notif_tx(None);
        rsp_tx.send(result).unwrap();
    }

    async fn shutdown(self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        match self.inner {
            FatCrabMakerInnerActor::Buy(buy_actor) => {
                buy_actor.data.terminate().await;
            }
            FatCrabMakerInnerActor::Sell(sell_actor) => {
                sell_actor.data.terminate().await;
            }
        }

        match self.n3xb_maker.shutdown().await {
            Ok(_) => {
                rsp_tx.send(Ok(())).unwrap();
            }
            Err(error) => {
                rsp_tx.send(Err(error.into())).unwrap();
            }
        }
    }

    async fn handle_offer_notif(&self, offer_envelope: OfferEnvelope) {
        let n3xb_offer = offer_envelope.offer.clone();
        match FatCrabOffer::validate_n3xb_offer(n3xb_offer) {
            Ok(_) => {
                if let Some(notif_tx) = &self.notif_tx {
                    let fatcrab_offer_envelope = FatCrabOfferEnvelope {
                        envelope: offer_envelope,
                    };

                    match self.inner {
                        FatCrabMakerInnerActor::Buy(ref buy_actor) => {
                            buy_actor
                                .data
                                .insert_offer_envelope(fatcrab_offer_envelope.clone())
                                .await;
                        }
                        FatCrabMakerInnerActor::Sell(ref sell_actor) => {
                            sell_actor
                                .data
                                .insert_offer_envelope(fatcrab_offer_envelope.clone())
                                .await;
                        }
                    }

                    notif_tx
                        .send(FatCrabMakerNotif::Offer(fatcrab_offer_envelope))
                        .await
                        .unwrap();
                } else {
                    warn!(
                        "Maker w/ TradeUUID {} do not have notif_tx registered",
                        self.trade_uuid.to_string()
                    );
                }
            }
            Err(error) => {
                error!(
                    "Maker w/ TradeUUID {} Offer Validation Error - {}",
                    self.trade_uuid.to_string(),
                    error.to_string()
                );
            }
        }
    }

    async fn handle_peer_notif(&mut self, peer_envelope: PeerEnvelope) {
        let fatcrab_peer_message = peer_envelope
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
                _envelope: peer_envelope,
                message: fatcrab_peer_message,
            };

            notif_tx
                .send(FatCrabMakerNotif::Peer(fatcrab_peer_envelope))
                .await
                .unwrap();
        } else {
            warn!(
                "Maker w/ TradeUUID {} do not have notif_tx registered",
                self.trade_uuid
            );
        }
    }
}

enum FatCrabMakerInnerActor {
    Buy(FatCrabMakerBuyActor),
    Sell(FatCrabMakerSellActor),
}

struct FatCrabMakerBuyActor {
    trade_uuid: Uuid,
    data: FatCrabMakerBuyData,
    n3xb_maker: MakerAccess,
    purse: PurseAccess,
    notif_tx: Option<mpsc::Sender<FatCrabMakerNotif>>,
}

impl FatCrabMakerBuyActor {
    async fn new(
        order: &FatCrabOrder,
        fatcrab_rx_addr: String,
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
        dir_path: impl AsRef<Path>,
    ) -> Self {
        let sats = order.amount * order.price;
        let btc_funds_id = purse.allocate_funds(sats as u64).await.unwrap();

        let data = FatCrabMakerBuyData::new(
            order.trade_uuid,
            purse.network,
            fatcrab_rx_addr,
            btc_funds_id,
            dir_path,
        )
        .await;

        Self {
            trade_uuid: order.trade_uuid,
            data,
            n3xb_maker,
            purse,
            notif_tx: None,
        }
    }

    async fn restore(
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<(Uuid, Self), FatCrabError> {
        let (trade_uuid, data) = FatCrabMakerBuyData::restore(purse.network, data_path).await?;
        let actor = Self {
            trade_uuid,
            data,
            n3xb_maker,
            purse,
            notif_tx: None,
        };
        Ok((trade_uuid, actor))
    }

    async fn trade_response(
        &self,
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
                    receive_address: self.data.fatcrab_rx_addr().await.clone(),
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

    async fn handle_peer_notif(&self, fatcrab_peer_message: &FatCrabPeerMessage) -> bool {
        // Should be recieving Fatcrab Tx ID along with BTC Rx Address here
        // Retain the BTC Rx Address, and notify User to confirm Fatcrab remittance
        let address = Address::from_str(&fatcrab_peer_message.receive_address).unwrap();
        let btc_addr = match address.require_network(self.purse.network) {
            Ok(address) => address,
            Err(error) => {
                error!(
                    "Maker w/ TradeUUID {} Address received from Peer {:?} is not {} - {}",
                    self.trade_uuid,
                    fatcrab_peer_message.receive_address,
                    self.purse.network,
                    error.to_string(),
                );
                return true;
            }
        };
        self.data.set_peer_btc_addr(btc_addr).await;
        return false;
    }

    async fn check_btc_tx_confirmation(&self, rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>) {
        rsp_tx
            .send(Err(FatCrabError::Simple {
                description: format!(
                    "Maker w/ TradeUUID {} is a Buyer, will not have a Peer BTC Txid to check on",
                    self.trade_uuid
                ),
            }))
            .unwrap();
    }

    async fn release_notify_peer(&self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        let btc_addr = match self.data.peer_btc_addr().await.clone() {
            Some(peer_btc_addr) => peer_btc_addr,
            None => {
                let error = FatCrabError::Simple {
                    description: format!(
                        "Maker w/ TradeUUID {} should have received BTC address from peer",
                        self.trade_uuid
                    ),
                };
                rsp_tx.send(Err(error)).unwrap();
                return;
            }
        };

        let txid = match self
            .purse
            .send_funds(self.data.btc_funds_id().await, btc_addr)
            .await
        {
            Ok(txid) => txid,
            Err(error) => {
                rsp_tx.send(Err(error.into())).unwrap();
                return;
            }
        };

        let message = FatCrabPeerMessage {
            receive_address: self.data.fatcrab_rx_addr().await.clone(),
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

    async fn trade_complete(self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        self.data.set_trade_completed().await;
        self.data.terminate().await;

        match self.n3xb_maker.trade_complete().await {
            Ok(_) => {
                rsp_tx.send(Ok(())).unwrap();
            }
            Err(error) => {
                rsp_tx.send(Err(error.into())).unwrap();
            }
        };
    }
}

struct FatCrabMakerSellActor {
    trade_uuid: Uuid,
    data: FatCrabMakerSellData,
    n3xb_maker: MakerAccess,
    purse: PurseAccess,
    notif_tx: Option<mpsc::Sender<FatCrabMakerNotif>>,
}

impl FatCrabMakerSellActor {
    async fn new(
        order: &FatCrabOrder,
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
        dir_path: impl AsRef<Path>,
    ) -> Self {
        let btc_rx_addr = purse.get_rx_address().await.unwrap();

        let data =
            FatCrabMakerSellData::new(order.trade_uuid, purse.network, btc_rx_addr, dir_path).await;

        Self {
            trade_uuid: order.trade_uuid,
            data,
            n3xb_maker,
            purse,
            notif_tx: None,
        }
    }

    async fn restore(
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<(Uuid, Self), FatCrabError> {
        let (trade_uuid, data) = FatCrabMakerSellData::restore(purse.network, data_path).await?;
        let actor = Self {
            trade_uuid,
            data,
            n3xb_maker,
            purse,
            notif_tx: None,
        };
        Ok((trade_uuid, actor))
    }

    async fn trade_response(
        &self,
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
                    receive_address: self.data.btc_rx_addr().await.to_string(),
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

    async fn handle_peer_notif(&self, fatcrab_peer_message: &FatCrabPeerMessage) -> bool {
        // Should be recieving BTC Txid along with Fatcrab Rx Address here
        // Retain and Notify User
        let txid = match Txid::from_str(&fatcrab_peer_message.txid) {
            Ok(txid) => txid,
            Err(error) => {
                error!(
                    "Maker w/ TradeUUID {} Txid received from Peer {:?} is not valid - {}",
                    self.trade_uuid,
                    fatcrab_peer_message.txid,
                    error.to_string()
                );
                return true;
            }
        };

        self.data.set_peer_btc_txid(txid).await;
        return false;
    }

    async fn check_btc_tx_confirmation(&self, rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>) {
        let txid = match self.data.peer_btc_txid().await {
            Some(txid) => txid,
            None => {
                let error = FatCrabError::Simple {
                    description: format!(
                        "Maker w/ TradeUUID {} should have received BTC Txid from peer",
                        self.trade_uuid
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
            receive_address: self.data.btc_rx_addr().await.to_string(),
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

    async fn trade_complete(self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        self.data.set_trade_completed().await;
        self.data.terminate().await;

        match self.n3xb_maker.trade_complete().await {
            Ok(_) => {
                rsp_tx.send(Ok(())).unwrap();
            }
            Err(error) => {
                rsp_tx.send(Err(error.into())).unwrap();
            }
        };
    }
}
