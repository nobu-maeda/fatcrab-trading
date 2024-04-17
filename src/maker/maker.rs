use std::{marker::PhantomData, path::Path, str::FromStr};
use tracing::{debug, error, trace, warn};

use bitcoin::{Address, Txid};
use crusty_n3xb::{
    maker::{MakerAccess, MakerNotif},
    offer::OfferEnvelope,
    peer_msg::PeerEnvelope,
    trade_rsp::{TradeResponseBuilder, TradeResponseStatus},
};
use tokio::{
    select,
    sync::{mpsc, oneshot},
    task::JoinError,
};
use uuid::Uuid;

use crate::{
    error::FatCrabError,
    offer::{FatCrabOffer, FatCrabOfferEnvelope},
    order::{FatCrabOrder, FatCrabOrderType},
    peer::{FatCrabPeerEnvelope, FatCrabPeerMessage},
    purse::PurseAccess,
    trade_rsp::{FatCrabMakeTradeRspSpecifics, FatCrabTradeRspType},
};

use super::{
    data::{FatCrabMakerBuyData, FatCrabMakerSellData},
    FatCrabMakerState,
};

pub struct FatCrabMakerNotifOfferStruct {
    pub state: FatCrabMakerState,
    pub offer_envelope: FatCrabOfferEnvelope,
}

pub struct FatCrabMakerNotifPeerStruct {
    pub state: FatCrabMakerState,
    pub peer_envelope: FatCrabPeerEnvelope,
}

pub enum FatCrabMakerNotif {
    Offer(FatCrabMakerNotifOfferStruct),
    Peer(FatCrabMakerNotifPeerStruct),
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

impl FatCrabMakerAccessEnum {
    pub async fn shutdown(&self) -> Result<(), FatCrabError> {
        match self {
            FatCrabMakerAccessEnum::Buy(access) => access.shutdown().await,
            FatCrabMakerAccessEnum::Sell(access) => access.shutdown().await,
        }
    }
}

#[derive(Clone)]
pub struct FatCrabMakerAccess<OrderType = MakerBuy> {
    tx: mpsc::Sender<FatCrabMakerRequest>,
    _order_type: PhantomData<OrderType>,
}

impl FatCrabMakerAccess<MakerBuy> {
    pub async fn release_notify_peer(&self) -> Result<FatCrabMakerState, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<FatCrabMakerState, FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::ReleaseNotifyPeer {
                fatcrab_txid: None,
                rsp_tx,
            })
            .await?;
        rsp_rx.await.unwrap()
    }
}

impl FatCrabMakerAccess<MakerSell> {
    pub async fn get_peer_btc_txid(&self) -> Result<Option<String>, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<Option<String>, FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::GetPeerBtcTxid { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn check_btc_tx_confirmation(&self) -> Result<u32, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<u32, FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::CheckBtcTxConf { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn notify_peer(
        &self,
        fatcrab_txid: impl Into<String>,
    ) -> Result<FatCrabMakerState, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<FatCrabMakerState, FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::ReleaseNotifyPeer {
                fatcrab_txid: Some(fatcrab_txid.into()),
                rsp_tx,
            })
            .await?;
        rsp_rx.await.unwrap()
    }
}

impl<OrderType> FatCrabMakerAccess<OrderType> {
    pub async fn post_new_order(&self) -> Result<FatCrabMakerState, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<FatCrabMakerState, FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::PostNewOrder { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn get_order_details(&self) -> Result<FatCrabOrder, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<FatCrabOrder, FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::GetOrderDetails { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn get_state(&self) -> Result<FatCrabMakerState, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<FatCrabMakerState, FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::GetState { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn get_peer_pubkey(&self) -> Result<Option<String>, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<Option<String>, FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::GetPeerPubkey { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn query_offers(&self) -> Result<Vec<FatCrabOfferEnvelope>, FatCrabError> {
        let (rsp_tx, rsp_rx) =
            oneshot::channel::<Result<Vec<FatCrabOfferEnvelope>, FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::QueryOffers { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn query_peer_msg(&self) -> Result<Option<FatCrabPeerEnvelope>, FatCrabError> {
        let (rsp_tx, rsp_rx) =
            oneshot::channel::<Result<Option<FatCrabPeerEnvelope>, FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::QueryPeerMsg { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn cancel_order(&self) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::CancelOrder { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn trade_response(
        &self,
        trade_rsp_type: FatCrabTradeRspType,
        offer_envelope: FatCrabOfferEnvelope,
    ) -> Result<FatCrabMakerState, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<FatCrabMakerState, FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::TradeResponse {
                trade_rsp_type,
                offer_envelope,
                rsp_tx,
            })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn trade_complete(&self) -> Result<FatCrabMakerState, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<FatCrabMakerState, FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::TradeComplete { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn shutdown(&self) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::Shutdown { rsp_tx })
            .await?;
        rsp_rx.await?
    }

    pub async fn register_notif_tx(
        &self,
        tx: mpsc::Sender<FatCrabMakerNotif>,
    ) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::RegisterNotifTx { tx, rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn unregister_notif_tx(&self) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabMakerRequest::UnregisterNotifTx { rsp_tx })
            .await?;
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
    ) -> Result<Self, FatCrabError> {
        assert_eq!(order.order_type, FatCrabOrderType::Buy);
        let (tx, rx) =
            mpsc::channel::<FatCrabMakerRequest>(FatCrabMaker::MAKE_TRADE_REQUEST_CHANNEL_SIZE);

        let actor_result = FatCrabMakerActor::new(
            rx,
            order.to_owned(),
            Some(fatcrab_rx_addr.into()),
            n3xb_maker,
            purse,
            dir_path,
        )
        .await;

        let actor = match actor_result {
            Ok(actor) => actor,
            Err(error) => {
                return Err(error);
            }
        };

        let task_handle = tokio::spawn(async move { actor.run().await });

        let maker = Self {
            tx,
            task_handle,
            _order_type: PhantomData,
        };

        Ok(maker)
    }

    pub(crate) fn restore(
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let (tx, rx) =
            mpsc::channel::<FatCrabMakerRequest>(FatCrabMaker::MAKE_TRADE_REQUEST_CHANNEL_SIZE);

        let actor = FatCrabMakerActor::restore_buy_actor(rx, n3xb_maker, purse, data_path)?;
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
    ) -> Result<Self, FatCrabError> {
        assert_eq!(order.order_type, FatCrabOrderType::Sell);
        let (tx, rx) =
            mpsc::channel::<FatCrabMakerRequest>(FatCrabMaker::MAKE_TRADE_REQUEST_CHANNEL_SIZE);

        let actor_result =
            FatCrabMakerActor::new(rx, order.to_owned(), None, n3xb_maker, purse, dir_path).await;

        let actor = match actor_result {
            Ok(actor) => actor,
            Err(error) => {
                return Err(error);
            }
        };

        let task_handle = tokio::spawn(async move { actor.run().await });

        let maker = Self {
            tx,
            task_handle,
            _order_type: PhantomData,
        };

        Ok(maker)
    }

    pub(crate) fn restore(
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let (tx, rx) =
            mpsc::channel::<FatCrabMakerRequest>(FatCrabMaker::MAKE_TRADE_REQUEST_CHANNEL_SIZE);

        let actor = FatCrabMakerActor::restore_sell_actor(rx, n3xb_maker, purse, data_path)?;
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
        rsp_tx: oneshot::Sender<Result<FatCrabMakerState, FatCrabError>>,
    },
    GetState {
        rsp_tx: oneshot::Sender<Result<FatCrabMakerState, FatCrabError>>,
    },
    GetOrderDetails {
        rsp_tx: oneshot::Sender<Result<FatCrabOrder, FatCrabError>>,
    },
    GetPeerPubkey {
        rsp_tx: oneshot::Sender<Result<Option<String>, FatCrabError>>,
    },
    GetPeerBtcTxid {
        rsp_tx: oneshot::Sender<Result<Option<String>, FatCrabError>>,
    },
    QueryOffers {
        rsp_tx: oneshot::Sender<Result<Vec<FatCrabOfferEnvelope>, FatCrabError>>,
    },
    QueryPeerMsg {
        rsp_tx: oneshot::Sender<Result<Option<FatCrabPeerEnvelope>, FatCrabError>>,
    },
    CancelOrder {
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    TradeResponse {
        trade_rsp_type: FatCrabTradeRspType,
        offer_envelope: FatCrabOfferEnvelope,
        rsp_tx: oneshot::Sender<Result<FatCrabMakerState, FatCrabError>>,
    },
    ReleaseNotifyPeer {
        fatcrab_txid: Option<String>,
        rsp_tx: oneshot::Sender<Result<FatCrabMakerState, FatCrabError>>,
    },
    CheckBtcTxConf {
        rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>,
    },
    TradeComplete {
        rsp_tx: oneshot::Sender<Result<FatCrabMakerState, FatCrabError>>,
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
    ) -> Result<Self, FatCrabError> {
        let inner = match order.order_type {
            FatCrabOrderType::Buy => {
                let buy_actor_result = FatCrabMakerBuyActor::new(
                    &order,
                    fatcrab_rx_addr.unwrap(),
                    n3xb_maker.clone(),
                    purse,
                    dir_path,
                )
                .await;

                let buy_actor = match buy_actor_result {
                    Ok(buy_actor) => buy_actor,
                    Err(error) => {
                        return Err(error);
                    }
                };

                FatCrabMakerInnerActor::Buy(buy_actor)
            }

            FatCrabOrderType::Sell => {
                let sell_actor =
                    FatCrabMakerSellActor::new(&order, n3xb_maker.clone(), purse, dir_path).await;
                FatCrabMakerInnerActor::Sell(sell_actor)
            }
        };

        let maker_actor = Self {
            inner,
            rx,
            trade_uuid: order.trade_uuid,
            notif_tx: None,
            n3xb_maker,
        };

        Ok(maker_actor)
    }

    fn restore_buy_actor(
        rx: mpsc::Receiver<FatCrabMakerRequest>,
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let (trade_uuid, actor) =
            FatCrabMakerBuyActor::restore(n3xb_maker.clone(), purse, data_path)?;
        let inner = FatCrabMakerInnerActor::Buy(actor);

        Ok(Self {
            inner,
            trade_uuid,
            rx,
            notif_tx: None,
            n3xb_maker,
        })
    }

    fn restore_sell_actor(
        rx: mpsc::Receiver<FatCrabMakerRequest>,
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let (trade_uuid, actor) =
            FatCrabMakerSellActor::restore(n3xb_maker.clone(), purse, data_path)?;
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
                        FatCrabMakerRequest::GetOrderDetails { rsp_tx } => {
                            self.get_order_details(rsp_tx);
                        },
                        FatCrabMakerRequest::GetState { rsp_tx } => {
                            self.get_state(rsp_tx);
                        },
                        FatCrabMakerRequest::GetPeerPubkey { rsp_tx } => {
                            self.get_peer_pubkey(rsp_tx);
                        },
                        FatCrabMakerRequest::GetPeerBtcTxid { rsp_tx } => {
                            self.get_peer_btc_txid(rsp_tx);
                        },
                        FatCrabMakerRequest::QueryOffers { rsp_tx } => {
                            self.query_offers(rsp_tx);
                        },
                        FatCrabMakerRequest::QueryPeerMsg { rsp_tx } => {
                            self.query_peer_msg(rsp_tx);
                        },
                        FatCrabMakerRequest::CancelOrder { rsp_tx } => {
                            match self.inner {
                                FatCrabMakerInnerActor::Buy(ref buy_actor) => {
                                    buy_actor.cancel_order(rsp_tx).await;
                                },
                                FatCrabMakerInnerActor::Sell(ref sell_actor) => {
                                    sell_actor.cancel_order(rsp_tx).await;
                                }
                            }
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
                                    buy_actor.check_btc_tx_confirmation(rsp_tx);
                                },
                                FatCrabMakerInnerActor::Sell(ref sell_actor) => {
                                    sell_actor.check_btc_tx_confirmation(rsp_tx).await;
                                }
                            }
                        },
                        FatCrabMakerRequest::TradeComplete { rsp_tx } => {
                            match self.inner {
                                FatCrabMakerInnerActor::Buy(ref buy_actor) => {
                                    buy_actor.trade_complete(rsp_tx).await;
                                },
                                FatCrabMakerInnerActor::Sell(ref sell_actor) => {
                                    sell_actor.trade_complete(rsp_tx).await;
                                }
                            }
                        },
                        FatCrabMakerRequest::Shutdown { rsp_tx } => {
                            self.shutdown(rsp_tx).await;
                            return;
                        },
                        FatCrabMakerRequest::RegisterNotifTx { tx, rsp_tx } => {
                            self.register_notif_tx(tx, rsp_tx);
                        },
                        FatCrabMakerRequest::UnregisterNotifTx { rsp_tx } => {
                            self.unregister_notif_tx(rsp_tx);
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

    async fn post_new_order(
        &mut self,
        rsp_tx: oneshot::Sender<Result<FatCrabMakerState, FatCrabError>>,
    ) {
        if let Some(error) = self.n3xb_maker.post_new_order().await.err() {
            rsp_tx.send(Err(error.into())).unwrap();
        } else {
            rsp_tx
                .send(Ok(FatCrabMakerState::WaitingForOffers))
                .unwrap();
            self.set_state(FatCrabMakerState::WaitingForOffers);
        }
    }

    fn get_order_details(&self, rsp_tx: oneshot::Sender<Result<FatCrabOrder, FatCrabError>>) {
        let order = match self.inner {
            FatCrabMakerInnerActor::Buy(ref buy_actor) => buy_actor.data.order(),
            FatCrabMakerInnerActor::Sell(ref sell_actor) => sell_actor.data.order(),
        };
        rsp_tx.send(Ok(order)).unwrap();
    }

    fn set_state(&self, state: FatCrabMakerState) {
        debug!(
            "Setting Maker w/ TradeUUID {} State to {:?}",
            self.trade_uuid, state
        );

        match self.inner {
            FatCrabMakerInnerActor::Buy(ref buy_actor) => {
                buy_actor.data.set_state(state);
            }
            FatCrabMakerInnerActor::Sell(ref sell_actor) => {
                sell_actor.data.set_state(state);
            }
        }
    }

    fn get_state(&self, rsp_tx: oneshot::Sender<Result<FatCrabMakerState, FatCrabError>>) {
        let state = match self.inner {
            FatCrabMakerInnerActor::Buy(ref buy_actor) => buy_actor.data.state(),
            FatCrabMakerInnerActor::Sell(ref sell_actor) => sell_actor.data.state(),
        };
        rsp_tx.send(Ok(state)).unwrap();
    }

    fn get_peer_pubkey(&self, rsp_tx: oneshot::Sender<Result<Option<String>, FatCrabError>>) {
        let pubkey = match self.inner {
            FatCrabMakerInnerActor::Buy(ref buy_actor) => buy_actor.data.peer_pubkey(),
            FatCrabMakerInnerActor::Sell(ref sell_actor) => sell_actor.data.peer_pubkey(),
        };
        rsp_tx.send(Ok(pubkey)).unwrap();
    }

    fn get_peer_btc_txid(&self, rsp_tx: oneshot::Sender<Result<Option<String>, FatCrabError>>) {
        match self.inner {
            FatCrabMakerInnerActor::Buy(ref _buy_actor) => {
                let error = FatCrabError::Simple {
                    description: "Buy Maker does not have peer_btc_txid".to_string(),
                };
                rsp_tx.send(Err(error)).unwrap();
            }
            FatCrabMakerInnerActor::Sell(ref sell_actor) => {
                let peer_btc_txid = match sell_actor.data.peer_btc_txid() {
                    Some(txid) => Some(txid.to_string()),
                    None => None,
                };
                rsp_tx.send(Ok(peer_btc_txid)).unwrap();
            }
        };
    }

    fn query_offers(
        &self,
        rsp_tx: oneshot::Sender<Result<Vec<FatCrabOfferEnvelope>, FatCrabError>>,
    ) {
        let offer_envelopes = match self.inner {
            FatCrabMakerInnerActor::Buy(ref buy_actor) => buy_actor.data.offer_envelopes(),
            FatCrabMakerInnerActor::Sell(ref sell_actor) => sell_actor.data.offer_envelopes(),
        };
        rsp_tx.send(Ok(offer_envelopes)).unwrap();
    }

    fn query_peer_msg(
        &self,
        rsp_tx: oneshot::Sender<Result<Option<FatCrabPeerEnvelope>, FatCrabError>>,
    ) {
        let peer_envelope = match self.inner {
            FatCrabMakerInnerActor::Buy(ref buy_actor) => buy_actor.data.peer_envelope(),
            FatCrabMakerInnerActor::Sell(ref sell_actor) => sell_actor.data.peer_envelope(),
        };
        rsp_tx.send(Ok(peer_envelope)).unwrap();
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

    fn register_notif_tx(
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

    fn unregister_notif_tx(&mut self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
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
                buy_actor.data.terminate();
            }
            FatCrabMakerInnerActor::Sell(sell_actor) => {
                sell_actor.data.terminate();
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
                        pubkey: offer_envelope.pubkey.to_string(),
                        envelope: offer_envelope,
                    };

                    match self.inner {
                        FatCrabMakerInnerActor::Buy(ref buy_actor) => {
                            buy_actor
                                .data
                                .insert_offer_envelope(fatcrab_offer_envelope.clone());
                            buy_actor.data.set_state(FatCrabMakerState::ReceivedOffer);
                        }
                        FatCrabMakerInnerActor::Sell(ref sell_actor) => {
                            sell_actor
                                .data
                                .insert_offer_envelope(fatcrab_offer_envelope.clone());
                            sell_actor.data.set_state(FatCrabMakerState::ReceivedOffer);
                        }
                    }

                    let offer_notif = FatCrabMakerNotifOfferStruct {
                        state: FatCrabMakerState::ReceivedOffer,
                        offer_envelope: fatcrab_offer_envelope,
                    };

                    notif_tx
                        .send(FatCrabMakerNotif::Offer(offer_notif))
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

        let fatcrab_peer_envelope = FatCrabPeerEnvelope {
            _envelope: peer_envelope,
            message: fatcrab_peer_message.clone(),
        };

        let new_state: FatCrabMakerState;

        match self.inner {
            FatCrabMakerInnerActor::Buy(ref mut buy_actor) => {
                if buy_actor.handle_peer_notif(&fatcrab_peer_message) {
                    return;
                }
                buy_actor
                    .data
                    .set_peer_envelope(fatcrab_peer_envelope.clone());
                new_state = FatCrabMakerState::InboundFcNotified;
            }
            FatCrabMakerInnerActor::Sell(ref mut sell_actor) => {
                if sell_actor.handle_peer_notif(&fatcrab_peer_message) {
                    return;
                }
                sell_actor
                    .data
                    .set_peer_envelope(fatcrab_peer_envelope.clone());
                new_state = FatCrabMakerState::InboundBtcNotified;
            }
        }

        self.set_state(new_state.clone());

        if let Some(notif_tx) = &self.notif_tx {
            let peer_notif = FatCrabMakerNotifPeerStruct {
                state: new_state,
                peer_envelope: fatcrab_peer_envelope,
            };

            notif_tx
                .send(FatCrabMakerNotif::Peer(peer_notif))
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
    ) -> Result<Self, FatCrabError> {
        let sats = order.amount * order.price;
        let funding_result = purse.allocate_funds(sats as u64).await;

        let btc_funds_id = match funding_result {
            Ok(btc_funds_id) => btc_funds_id,
            Err(error) => {
                return Err(error);
            }
        };

        let data = FatCrabMakerBuyData::new(
            order,
            purse.network,
            fatcrab_rx_addr,
            btc_funds_id,
            dir_path,
        );

        let maker_buy_actor = Self {
            trade_uuid: order.trade_uuid,
            data,
            n3xb_maker,
            purse,
            notif_tx: None,
        };

        Ok(maker_buy_actor)
    }

    fn restore(
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<(Uuid, Self), FatCrabError> {
        let (trade_uuid, data) = FatCrabMakerBuyData::restore(purse.network, data_path)?;
        let actor = Self {
            trade_uuid,
            data,
            n3xb_maker,
            purse,
            notif_tx: None,
        };
        Ok((trade_uuid, actor))
    }

    async fn cancel_order(&self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        match self.data.state() {
            FatCrabMakerState::New
            | FatCrabMakerState::WaitingForOffers
            | FatCrabMakerState::ReceivedOffer => match self.n3xb_maker.cancel_order().await {
                Ok(_) => {
                    self.data.set_state(FatCrabMakerState::TradeCompleted);
                    rsp_tx.send(Ok(())).unwrap();
                }
                Err(error) => {
                    rsp_tx.send(Err(error.into())).unwrap();
                }
            },
            _ => {
                let error = FatCrabError::Simple {
                    description: format!(
                        "Maker w/ TradeUUID {} cannot cancel order in state {:?}",
                        self.trade_uuid,
                        self.data.state()
                    ),
                };
                rsp_tx.send(Err(error)).unwrap();
            }
        }
    }

    async fn trade_response(
        &self,
        trade_rsp_type: FatCrabTradeRspType,
        offer_envelope: FatCrabOfferEnvelope,
        rsp_tx: oneshot::Sender<Result<FatCrabMakerState, FatCrabError>>,
    ) {
        match trade_rsp_type {
            FatCrabTradeRspType::Accept => {
                let mut trade_rsp_builder = TradeResponseBuilder::new();
                trade_rsp_builder.offer_event_id(offer_envelope.envelope.event_id);
                trade_rsp_builder.trade_response(TradeResponseStatus::Accepted);

                let trade_engine_specifics = FatCrabMakeTradeRspSpecifics {
                    receive_address: self.data.fatcrab_rx_addr().clone(),
                };
                trade_rsp_builder.trade_engine_specifics(Box::new(trade_engine_specifics));

                let n3xb_trade_rsp = trade_rsp_builder.build().unwrap();
                self.n3xb_maker.accept_offer(n3xb_trade_rsp).await.unwrap();

                self.data.set_peer_pubkey(offer_envelope.pubkey);
                self.data.set_state(FatCrabMakerState::AcceptedOffer);
                rsp_tx.send(Ok(FatCrabMakerState::AcceptedOffer)).unwrap();
            }
            FatCrabTradeRspType::Reject => {
                let mut trade_rsp_builder = TradeResponseBuilder::new();
                trade_rsp_builder.offer_event_id(offer_envelope.envelope.event_id);
                trade_rsp_builder.trade_response(TradeResponseStatus::Rejected);

                let n3xb_trade_rsp = trade_rsp_builder.build().unwrap();
                self.n3xb_maker.reject_offer(n3xb_trade_rsp).await.unwrap();

                let state = self.data.state();
                rsp_tx.send(Ok(state)).unwrap();
            }
        }
    }

    fn handle_peer_notif(&self, fatcrab_peer_message: &FatCrabPeerMessage) -> bool {
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
        self.data.set_peer_btc_addr(btc_addr);
        return false;
    }

    fn check_btc_tx_confirmation(&self, rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>) {
        rsp_tx
            .send(Err(FatCrabError::Simple {
                description: format!(
                    "Maker w/ TradeUUID {} is a Buyer, will not have a Peer BTC Txid to check on",
                    self.trade_uuid
                ),
            }))
            .unwrap();
    }

    async fn release_notify_peer(
        &self,
        rsp_tx: oneshot::Sender<Result<FatCrabMakerState, FatCrabError>>,
    ) {
        trace!(
            "Maker w/ TradeUUID {} Release BTC & Notify Peer",
            self.trade_uuid
        );

        let btc_addr = match self.data.peer_btc_addr().clone() {
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
            .send_funds(self.data.btc_funds_id(), btc_addr)
            .await
        {
            Ok(txid) => txid,
            Err(error) => {
                rsp_tx.send(Err(error.into())).unwrap();
                return;
            }
        };

        let message = FatCrabPeerMessage {
            receive_address: self.data.fatcrab_rx_addr().clone(),
            txid: txid.to_string(),
        };
        match self.n3xb_maker.send_peer_message(Box::new(message)).await {
            Ok(_) => {
                rsp_tx
                    .send(Ok(FatCrabMakerState::NotifiedOutbound))
                    .unwrap();
                self.data.set_state(FatCrabMakerState::NotifiedOutbound);
            }
            Err(error) => {
                rsp_tx.send(Err(error.into())).unwrap();
            }
        }
    }

    async fn trade_complete(
        &self,
        rsp_tx: oneshot::Sender<Result<FatCrabMakerState, FatCrabError>>,
    ) {
        let new_state = FatCrabMakerState::TradeCompleted;
        self.data.set_state(new_state.clone());

        match self.n3xb_maker.trade_complete().await {
            Ok(_) => {
                rsp_tx.send(Ok(new_state)).unwrap();
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

        let data = FatCrabMakerSellData::new(order, purse.network, btc_rx_addr, dir_path);

        Self {
            trade_uuid: order.trade_uuid,
            data,
            n3xb_maker,
            purse,
            notif_tx: None,
        }
    }

    fn restore(
        n3xb_maker: MakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<(Uuid, Self), FatCrabError> {
        let (trade_uuid, data) = FatCrabMakerSellData::restore(purse.network, data_path)?;
        let actor = Self {
            trade_uuid,
            data,
            n3xb_maker,
            purse,
            notif_tx: None,
        };
        Ok((trade_uuid, actor))
    }

    async fn cancel_order(&self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        match self.data.state() {
            FatCrabMakerState::New
            | FatCrabMakerState::WaitingForOffers
            | FatCrabMakerState::ReceivedOffer => match self.n3xb_maker.cancel_order().await {
                Ok(_) => {
                    self.data.set_state(FatCrabMakerState::TradeCompleted);
                    rsp_tx.send(Ok(())).unwrap();
                }
                Err(error) => {
                    rsp_tx.send(Err(error.into())).unwrap();
                }
            },
            _ => {
                let error = FatCrabError::Simple {
                    description: format!(
                        "Maker w/ TradeUUID {} cannot cancel order in state {:?}",
                        self.trade_uuid,
                        self.data.state()
                    ),
                };
                rsp_tx.send(Err(error)).unwrap();
            }
        }
    }

    async fn trade_response(
        &self,
        trade_rsp_type: FatCrabTradeRspType,
        offer_envelope: FatCrabOfferEnvelope,
        rsp_tx: oneshot::Sender<Result<FatCrabMakerState, FatCrabError>>,
    ) {
        match trade_rsp_type {
            FatCrabTradeRspType::Accept => {
                let mut trade_rsp_builder = TradeResponseBuilder::new();
                trade_rsp_builder.offer_event_id(offer_envelope.envelope.event_id);
                trade_rsp_builder.trade_response(TradeResponseStatus::Accepted);

                let trade_engine_specifics = FatCrabMakeTradeRspSpecifics {
                    receive_address: self.data.btc_rx_addr().to_string(),
                };
                trade_rsp_builder.trade_engine_specifics(Box::new(trade_engine_specifics));

                let n3xb_trade_rsp = trade_rsp_builder.build().unwrap();
                self.n3xb_maker.accept_offer(n3xb_trade_rsp).await.unwrap();

                self.data.set_peer_pubkey(offer_envelope.pubkey);
                self.data.set_state(FatCrabMakerState::AcceptedOffer);
                rsp_tx.send(Ok(FatCrabMakerState::AcceptedOffer)).unwrap();
            }
            FatCrabTradeRspType::Reject => {
                let mut trade_rsp_builder = TradeResponseBuilder::new();
                trade_rsp_builder.offer_event_id(offer_envelope.envelope.event_id);
                trade_rsp_builder.trade_response(TradeResponseStatus::Rejected);

                let n3xb_trade_rsp = trade_rsp_builder.build().unwrap();
                self.n3xb_maker.reject_offer(n3xb_trade_rsp).await.unwrap();

                let state = self.data.state();
                rsp_tx.send(Ok(state)).unwrap();
            }
        }
    }

    fn handle_peer_notif(&self, fatcrab_peer_message: &FatCrabPeerMessage) -> bool {
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

        self.data.set_peer_btc_txid(txid);
        return false;
    }

    async fn check_btc_tx_confirmation(&self, rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>) {
        let txid = match self.data.peer_btc_txid() {
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

    async fn notify_peer(
        &self,
        txid: String,
        rsp_tx: oneshot::Sender<Result<FatCrabMakerState, FatCrabError>>,
    ) {
        let message = FatCrabPeerMessage {
            receive_address: self.data.btc_rx_addr().to_string(),
            txid,
        };

        match self.n3xb_maker.send_peer_message(Box::new(message)).await {
            Ok(_) => {
                rsp_tx
                    .send(Ok(FatCrabMakerState::NotifiedOutbound))
                    .unwrap();
                self.data.set_state(FatCrabMakerState::NotifiedOutbound);
            }
            Err(error) => {
                rsp_tx.send(Err(error.into())).unwrap();
            }
        }
    }

    async fn trade_complete(
        &self,
        rsp_tx: oneshot::Sender<Result<FatCrabMakerState, FatCrabError>>,
    ) {
        let new_state = FatCrabMakerState::TradeCompleted;

        self.data.set_state(new_state.clone());

        match self.n3xb_maker.trade_complete().await {
            Ok(_) => {
                rsp_tx.send(Ok(new_state)).unwrap();
            }
            Err(error) => {
                rsp_tx.send(Err(error.into())).unwrap();
            }
        };
    }
}
