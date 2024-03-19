use log::{error, warn};
use std::{marker::PhantomData, path::Path, str::FromStr};

use bitcoin::{address::Address, Txid};
use crusty_n3xb::{
    peer_msg::PeerEnvelope,
    taker::{TakerAccess, TakerNotif},
    trade_rsp::TradeResponseEnvelope,
};
use tokio::{
    select,
    sync::{mpsc, oneshot},
    task::JoinError,
};
use uuid::Uuid;

use crate::{
    error::FatCrabError,
    order::{FatCrabOrderEnvelope, FatCrabOrderType},
    peer::{FatCrabPeerEnvelope, FatCrabPeerMessage},
    purse::PurseAccess,
    trade_rsp::{FatCrabTradeRsp, FatCrabTradeRspEnvelope},
};

use super::{
    data::{FatCrabTakerBuyData, FatCrabTakerSellData},
    FatCrabTakerState,
};

pub struct FatCrabTakerNotifTradeRspStruct {
    pub state: FatCrabTakerState,
    pub trade_rsp_envelope: FatCrabTradeRspEnvelope,
}

pub struct FatCrabTakerNotifPeerStruct {
    pub state: FatCrabTakerState,
    pub peer_envelope: FatCrabPeerEnvelope,
}

pub enum FatCrabTakerNotif {
    TradeRsp(FatCrabTakerNotifTradeRspStruct),
    Peer(FatCrabTakerNotifPeerStruct),
}

#[derive(Clone)]
// Just for purpose of typing the Taker
pub struct TakerBuy {} //  Means Taker is taking a Buy Order to Sell

#[derive(Clone)]
pub struct TakerSell {} // Means Tkaer is taking a Sell Order to Buy

#[derive(Clone)]
pub enum FatCrabTakerAccessEnum {
    Buy(FatCrabTakerAccess<TakerBuy>),
    Sell(FatCrabTakerAccess<TakerSell>),
}

impl FatCrabTakerAccessEnum {
    pub async fn shutdown(&self) -> Result<(), FatCrabError> {
        match self {
            FatCrabTakerAccessEnum::Buy(access) => access.shutdown().await,
            FatCrabTakerAccessEnum::Sell(access) => access.shutdown().await,
        }
    }
}

#[derive(Clone)]
pub struct FatCrabTakerAccess<OrderType = TakerBuy> {
    tx: mpsc::Sender<FatCrabTakerRequest>,
    _order_type: PhantomData<OrderType>,
}

impl FatCrabTakerAccess<TakerBuy> {
    // Means Taker is taking a Buy Order to Sell
    pub async fn notify_peer(
        &self,
        txid: impl Into<String>,
    ) -> Result<FatCrabTakerState, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<FatCrabTakerState, FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::NotifyPeer {
                txid: txid.into(),
                rsp_tx,
            })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn query_trade_rsp(&self) -> Result<Option<FatCrabTradeRspEnvelope>, FatCrabError> {
        let (rsp_tx, rsp_rx) =
            oneshot::channel::<Result<Option<FatCrabTradeRspEnvelope>, FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::QueryTradeRsp { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn check_btc_tx_confirmation(&self) -> Result<u32, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<u32, FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::CheckBtcTxConf { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }
}

impl FatCrabTakerAccess<TakerSell> {}

impl<OrderType> FatCrabTakerAccess<OrderType> {
    pub async fn take_order(&self) -> Result<FatCrabTakerState, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<FatCrabTakerState, FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::TakeOrder { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn get_order_details(&self) -> Result<FatCrabOrderEnvelope, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<FatCrabOrderEnvelope, FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::GetOrderDetails { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn get_state(&self) -> Result<FatCrabTakerState, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<FatCrabTakerState, FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::GetState { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn query_peer_msg(&self) -> Result<Option<FatCrabPeerEnvelope>, FatCrabError> {
        let (rsp_tx, rsp_rx) =
            oneshot::channel::<Result<Option<FatCrabPeerEnvelope>, FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::QueryPeerMsg { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn trade_complete(&self) -> Result<FatCrabTakerState, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<FatCrabTakerState, FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::TradeComplete { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn shutdown(&self) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::Shutdown { rsp_tx })
            .await?;
        rsp_rx.await?
    }

    pub async fn register_notif_tx(
        &self,
        tx: mpsc::Sender<FatCrabTakerNotif>,
    ) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::RegisterNotifTx { tx, rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }

    pub async fn unregister_notif_tx(&self) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::UnregisterNotifTx { rsp_tx })
            .await?;
        rsp_rx.await.unwrap()
    }
}

pub(crate) enum FatCrabTakerEnum {
    Buy(FatCrabTaker<TakerBuy>),
    Sell(FatCrabTaker<TakerSell>),
}

impl FatCrabTakerEnum {
    pub(crate) async fn await_task_handle(self) -> Result<(), JoinError> {
        match self {
            FatCrabTakerEnum::Buy(taker) => taker.task_handle.await,
            FatCrabTakerEnum::Sell(taker) => taker.task_handle.await,
        }
    }
}

pub(crate) struct FatCrabTaker<OrderType = TakerBuy> {
    tx: mpsc::Sender<FatCrabTakerRequest>,
    pub(crate) task_handle: tokio::task::JoinHandle<()>,
    _order_type: PhantomData<OrderType>,
}

impl FatCrabTaker {
    const TAKE_TRADE_REQUEST_CHANNEL_SIZE: usize = 10;
}

impl FatCrabTaker<TakerBuy> {
    // Means Taker is taking a Buy Order to Sell
    pub(crate) async fn new(
        order_envelope: &FatCrabOrderEnvelope,
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        dir_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        assert_eq!(order_envelope.order.order_type, FatCrabOrderType::Buy);
        let (tx, rx) =
            mpsc::channel::<FatCrabTakerRequest>(FatCrabTaker::TAKE_TRADE_REQUEST_CHANNEL_SIZE);
        let actor_result = FatCrabTakerActor::new(
            rx,
            order_envelope.to_owned(),
            None,
            n3xb_taker,
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

        let taker = Self {
            tx,
            task_handle,
            _order_type: PhantomData,
        };

        Ok(taker)
    }

    pub(crate) fn restore(
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let (tx, rx) =
            mpsc::channel::<FatCrabTakerRequest>(FatCrabTaker::TAKE_TRADE_REQUEST_CHANNEL_SIZE);

        let actor = FatCrabTakerActor::restore_buy_actor(rx, n3xb_taker, purse, data_path)?;
        let task_handle = tokio::spawn(async move { actor.run().await });

        Ok(Self {
            tx,
            task_handle,
            _order_type: PhantomData,
        })
    }

    pub(crate) fn new_accessor(&self) -> FatCrabTakerAccess<TakerBuy> {
        FatCrabTakerAccess {
            tx: self.tx.clone(),
            _order_type: PhantomData,
        }
    }
}

impl FatCrabTaker<TakerSell> {
    // Means Taker is taking a Sell Order to Buy
    pub(crate) async fn new(
        order_envelope: &FatCrabOrderEnvelope,
        fatcrab_rx_addr: impl Into<String>,
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        dir_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        assert_eq!(order_envelope.order.order_type, FatCrabOrderType::Sell);
        let (tx, rx) =
            mpsc::channel::<FatCrabTakerRequest>(FatCrabTaker::TAKE_TRADE_REQUEST_CHANNEL_SIZE);
        let actor_result = FatCrabTakerActor::new(
            rx,
            order_envelope.to_owned(),
            Some(fatcrab_rx_addr.into()),
            n3xb_taker,
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

        let taker = Self {
            tx,
            task_handle,
            _order_type: PhantomData,
        };

        Ok(taker)
    }

    pub(crate) fn restore(
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let (tx, rx) =
            mpsc::channel::<FatCrabTakerRequest>(FatCrabTaker::TAKE_TRADE_REQUEST_CHANNEL_SIZE);

        let actor = FatCrabTakerActor::restore_sell_actor(rx, n3xb_taker, purse, data_path)?;
        let task_handle = tokio::spawn(async move { actor.run().await });

        Ok(Self {
            tx,
            task_handle,
            _order_type: PhantomData,
        })
    }

    pub(crate) fn new_accessor(&self) -> FatCrabTakerAccess<TakerSell> {
        FatCrabTakerAccess {
            tx: self.tx.clone(),
            _order_type: PhantomData,
        }
    }
}

enum FatCrabTakerRequest {
    TakeOrder {
        rsp_tx: oneshot::Sender<Result<FatCrabTakerState, FatCrabError>>,
    },
    GetState {
        rsp_tx: oneshot::Sender<Result<FatCrabTakerState, FatCrabError>>,
    },
    GetOrderDetails {
        rsp_tx: oneshot::Sender<Result<FatCrabOrderEnvelope, FatCrabError>>,
    },
    QueryTradeRsp {
        rsp_tx: oneshot::Sender<Result<Option<FatCrabTradeRspEnvelope>, FatCrabError>>,
    },
    QueryPeerMsg {
        rsp_tx: oneshot::Sender<Result<Option<FatCrabPeerEnvelope>, FatCrabError>>,
    },
    NotifyPeer {
        txid: String,
        rsp_tx: oneshot::Sender<Result<FatCrabTakerState, FatCrabError>>,
    },
    CheckBtcTxConf {
        rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>,
    },
    TradeComplete {
        rsp_tx: oneshot::Sender<Result<FatCrabTakerState, FatCrabError>>,
    },
    Shutdown {
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
    inner: FatCrabTakerInnerActor,
    trade_uuid: Uuid,
    rx: mpsc::Receiver<FatCrabTakerRequest>,
    notif_tx: Option<mpsc::Sender<FatCrabTakerNotif>>,
    n3xb_taker: TakerAccess,
}

impl FatCrabTakerActor {
    async fn new(
        rx: mpsc::Receiver<FatCrabTakerRequest>,
        order_envelope: FatCrabOrderEnvelope,
        fatcrab_rx_addr: Option<String>,
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        dir_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let inner = match order_envelope.order.order_type {
            FatCrabOrderType::Buy => {
                let buy_actor =
                    FatCrabTakerBuyActor::new(&order_envelope, n3xb_taker.clone(), purse, dir_path)
                        .await;
                FatCrabTakerInnerActor::Buy(buy_actor)
            }

            FatCrabOrderType::Sell => {
                let sell_actor_result = FatCrabTakerSellActor::new(
                    &order_envelope,
                    fatcrab_rx_addr.unwrap(),
                    n3xb_taker.clone(),
                    purse,
                    dir_path,
                )
                .await;

                match sell_actor_result {
                    Ok(sell_actor) => FatCrabTakerInnerActor::Sell(sell_actor),
                    Err(error) => {
                        return Err(error);
                    }
                }
            }
        };

        let taker_actor = Self {
            inner,
            trade_uuid: order_envelope.order.trade_uuid,
            rx,
            notif_tx: None,
            n3xb_taker,
        };

        Ok(taker_actor)
    }

    fn restore_buy_actor(
        rx: mpsc::Receiver<FatCrabTakerRequest>,
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let (trade_uuid, actor) =
            FatCrabTakerBuyActor::restore(n3xb_taker.clone(), purse, data_path)?;
        let inner = FatCrabTakerInnerActor::Buy(actor);

        Ok(Self {
            inner,
            trade_uuid,
            rx,
            notif_tx: None,
            n3xb_taker,
        })
    }

    fn restore_sell_actor(
        rx: mpsc::Receiver<FatCrabTakerRequest>,
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let (trade_uuid, actor) =
            FatCrabTakerSellActor::restore(n3xb_taker.clone(), purse, data_path)?;
        let inner = FatCrabTakerInnerActor::Sell(actor);

        Ok(Self {
            inner,
            trade_uuid,
            rx,
            notif_tx: None,
            n3xb_taker,
        })
    }

    async fn run(mut self) {
        let (notif_tx, mut notif_rx) = mpsc::channel(5);

        self.n3xb_taker.register_notif_tx(notif_tx).await.unwrap();

        loop {
            select! {
                Some(request) = self.rx.recv() => {
                    match request {
                        FatCrabTakerRequest::TakeOrder { rsp_tx } => {
                            self.take_order(rsp_tx).await;
                        },
                        FatCrabTakerRequest::GetOrderDetails { rsp_tx } => {
                            self.get_order_details(rsp_tx);
                        },
                        FatCrabTakerRequest::GetState { rsp_tx } => {
                            self.get_state(rsp_tx);
                        },
                        FatCrabTakerRequest::QueryTradeRsp { rsp_tx } => {
                            match self.inner {
                                FatCrabTakerInnerActor::Buy(ref buy_actor) => {
                                    buy_actor.query_trade_rsp(rsp_tx);
                                }
                                FatCrabTakerInnerActor::Sell(ref sell_actor) => {
                                    sell_actor.query_trade_rsp(rsp_tx);
                                }
                            }
                        },
                        FatCrabTakerRequest::QueryPeerMsg { rsp_tx } => {
                            self.query_peer_msg(rsp_tx);
                        },
                        FatCrabTakerRequest::NotifyPeer { txid, rsp_tx } => {
                            match self.inner {
                                FatCrabTakerInnerActor::Buy(ref buy_actor) => {
                                    buy_actor.notify_peer(txid, rsp_tx).await;
                                }
                                FatCrabTakerInnerActor::Sell(ref sell_actor) => {
                                    sell_actor.notify_peer(txid, rsp_tx);
                                }
                            }
                        },
                        FatCrabTakerRequest::CheckBtcTxConf { rsp_tx } => {
                            match self.inner {
                                FatCrabTakerInnerActor::Buy(ref buy_actor) => {
                                    buy_actor.check_btc_tx_confirmation(rsp_tx).await;
                                }
                                FatCrabTakerInnerActor::Sell(ref sell_actor) => {
                                    sell_actor.check_btc_tx_confirmation(rsp_tx);
                                }
                            }
                        },
                        FatCrabTakerRequest::TradeComplete { rsp_tx } => {
                            match self.inner {
                                FatCrabTakerInnerActor::Buy(buy_actor) => {
                                    buy_actor.trade_complete(rsp_tx).await;
                                }
                                FatCrabTakerInnerActor::Sell(sell_actor) => {
                                    sell_actor.trade_complete(rsp_tx).await;
                                }
                            }
                            break;
                        },
                        FatCrabTakerRequest::Shutdown { rsp_tx } => {
                            self.shutdown(rsp_tx).await;
                            break;
                        },
                        FatCrabTakerRequest::RegisterNotifTx { tx, rsp_tx } => {
                            self.register_notif_tx(tx, rsp_tx);
                        },
                        FatCrabTakerRequest::UnregisterNotifTx { rsp_tx } => {
                            self.unregister_notif_tx(rsp_tx);
                        },
                    }
                },

                Some(result) = notif_rx.recv() => {
                    match result {
                        Ok(notif) => {
                            match notif {
                                TakerNotif::TradeRsp(trade_rsp_envelope) => {
                                    self.handle_trade_rsp_notif(trade_rsp_envelope).await;
                                },
                                TakerNotif::Peer(peer_envelope) => {
                                    self.handle_peer_notif(peer_envelope).await;
                                }
                            }
                        },
                        Err(error) => {
                            error!("Taker w/ TradeUUID {} Notif Rx Error - {}", self.trade_uuid, error.to_string());
                        }
                    }
                },
            }
        }
    }

    async fn take_order(&self, rsp_tx: oneshot::Sender<Result<FatCrabTakerState, FatCrabError>>) {
        if let Some(error) = self.n3xb_taker.take_order().await.err() {
            rsp_tx.send(Err(error.into())).unwrap();
        } else {
            let new_state = FatCrabTakerState::SubmittedOffer;
            rsp_tx.send(Ok(new_state.clone())).unwrap();
            self.set_state(new_state);
        }
    }

    fn get_order_details(
        &self,
        rsp_tx: oneshot::Sender<Result<FatCrabOrderEnvelope, FatCrabError>>,
    ) {
        let order_envelope = match self.inner {
            FatCrabTakerInnerActor::Buy(ref buy_actor) => buy_actor.data.order_envelope().clone(),
            FatCrabTakerInnerActor::Sell(ref sell_actor) => {
                sell_actor.data.order_envelope().clone()
            }
        };
        rsp_tx.send(Ok(order_envelope)).unwrap();
    }

    fn set_state(&self, state: FatCrabTakerState) {
        match self.inner {
            FatCrabTakerInnerActor::Buy(ref buy_actor) => {
                buy_actor.data.set_state(state);
            }
            FatCrabTakerInnerActor::Sell(ref sell_actor) => {
                sell_actor.data.set_state(state);
            }
        }
    }

    fn get_state(&self, rsp_tx: oneshot::Sender<Result<FatCrabTakerState, FatCrabError>>) {
        let state = match self.inner {
            FatCrabTakerInnerActor::Buy(ref buy_actor) => buy_actor.data.state(),
            FatCrabTakerInnerActor::Sell(ref sell_actor) => sell_actor.data.state(),
        };
        rsp_tx.send(Ok(state)).unwrap();
    }

    fn query_peer_msg(
        &self,
        rsp_tx: oneshot::Sender<Result<Option<FatCrabPeerEnvelope>, FatCrabError>>,
    ) {
        let peer_envelope = match self.inner {
            FatCrabTakerInnerActor::Buy(ref buy_actor) => buy_actor.data.peer_envelope(),
            FatCrabTakerInnerActor::Sell(ref sell_actor) => sell_actor.data.peer_envelope(),
        };
        rsp_tx.send(Ok(peer_envelope)).unwrap();
    }

    async fn shutdown(self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        match self.inner {
            FatCrabTakerInnerActor::Buy(buy_actor) => {
                buy_actor.data.terminate();
            }
            FatCrabTakerInnerActor::Sell(sell_actor) => {
                sell_actor.data.terminate();
            }
        }

        match self.n3xb_taker.shutdown().await {
            Ok(_) => {
                rsp_tx.send(Ok(())).unwrap();
            }
            Err(error) => {
                rsp_tx.send(Err(error.into())).unwrap();
            }
        }
    }

    fn set_notif_tx(&mut self, tx: Option<mpsc::Sender<FatCrabTakerNotif>>) {
        self.notif_tx = tx.clone();

        match self.inner {
            FatCrabTakerInnerActor::Buy(ref mut buy_actor) => {
                buy_actor.notif_tx = tx;
            }
            FatCrabTakerInnerActor::Sell(ref mut sell_actor) => {
                sell_actor.notif_tx = tx;
            }
        }
    }

    fn register_notif_tx(
        &mut self,
        tx: mpsc::Sender<FatCrabTakerNotif>,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    ) {
        let mut result = Ok(());
        if self.notif_tx.is_some() {
            let error = FatCrabError::Simple {
                description: format!(
                    "Taker w/ TradeUUID {} already have notif_tx registered",
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
                    "Taker w/ TradeUUID {} expected to already have notif_tx registered",
                    self.trade_uuid
                ),
            };
            result = Err(error);
        }
        self.set_notif_tx(None);
        rsp_tx.send(result).unwrap();
    }

    async fn handle_trade_rsp_notif(&mut self, trade_rsp_envelope: TradeResponseEnvelope) {
        let trade_rsp = FatCrabTradeRsp::from_n3xb_trade_rsp(trade_rsp_envelope.trade_rsp.clone());

        match self.inner {
            FatCrabTakerInnerActor::Buy(ref mut buy_actor) => {
                buy_actor
                    .handle_trade_rsp_notif(trade_rsp, trade_rsp_envelope)
                    .await;
            }
            FatCrabTakerInnerActor::Sell(ref mut sell_actor) => {
                sell_actor
                    .handle_trade_rsp_notif(trade_rsp, trade_rsp_envelope)
                    .await;
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

        match self.inner {
            FatCrabTakerInnerActor::Buy(ref mut buy_actor) => {
                buy_actor.handle_peer_notif(&fatcrab_peer_message);
                buy_actor
                    .data
                    .set_peer_envelope(fatcrab_peer_envelope.clone());
                buy_actor
                    .data
                    .set_state(FatCrabTakerState::InboundBtcNotified);
            }
            FatCrabTakerInnerActor::Sell(ref mut sell_actor) => {
                sell_actor.handle_peer_notif(&fatcrab_peer_message);
                sell_actor
                    .data
                    .set_peer_envelope(fatcrab_peer_envelope.clone());
                sell_actor
                    .data
                    .set_state(FatCrabTakerState::InboundFcNotified);
            }
        }

        let peer_notif = FatCrabTakerNotifPeerStruct {
            state: match self.inner {
                FatCrabTakerInnerActor::Buy(ref buy_actor) => buy_actor.data.state(),
                FatCrabTakerInnerActor::Sell(ref sell_actor) => sell_actor.data.state(),
            },
            peer_envelope: fatcrab_peer_envelope,
        };

        if let Some(notif_tx) = &self.notif_tx {
            notif_tx
                .send(FatCrabTakerNotif::Peer(peer_notif))
                .await
                .unwrap();
        } else {
            warn!(
                "Taker w/ TradeUUID {} do not have notif_tx registered",
                self.trade_uuid
            );
        }
    }
}

enum FatCrabTakerInnerActor {
    Buy(FatCrabTakerBuyActor),
    Sell(FatCrabTakerSellActor),
}

struct FatCrabTakerBuyActor {
    trade_uuid: Uuid,
    data: FatCrabTakerBuyData,
    n3xb_taker: TakerAccess,
    purse: PurseAccess,
    notif_tx: Option<mpsc::Sender<FatCrabTakerNotif>>,
}

impl FatCrabTakerBuyActor {
    async fn new(
        order_envelope: &FatCrabOrderEnvelope,
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        dir_path: impl AsRef<Path>,
    ) -> Self {
        let btc_rx_addr = purse.get_rx_address().await.unwrap();

        let data = FatCrabTakerBuyData::new(order_envelope, purse.network, btc_rx_addr, dir_path);

        Self {
            trade_uuid: order_envelope.order.trade_uuid,
            data,
            n3xb_taker,
            purse,
            notif_tx: None,
        }
    }

    fn restore(
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<(Uuid, Self), FatCrabError> {
        let (trade_uuid, data) = FatCrabTakerBuyData::restore(purse.network, data_path)?;
        let actor = Self {
            trade_uuid,
            data,
            n3xb_taker,
            purse,
            notif_tx: None,
        };
        Ok((trade_uuid, actor))
    }

    async fn handle_trade_rsp_notif(
        &self,
        trade_rsp: FatCrabTradeRsp,
        n3xb_trade_rsp_envelope: TradeResponseEnvelope,
    ) {
        if let Some(notif_tx) = &self.notif_tx {
            // Notify User to remite Fatcrab to Maker
            let fatcrab_trade_rsp_envelope = FatCrabTradeRspEnvelope {
                _envelope: n3xb_trade_rsp_envelope.clone(),
                trade_rsp: trade_rsp.clone(),
            };
            self.data
                .set_trade_rsp_envelope(fatcrab_trade_rsp_envelope.clone());

            match trade_rsp {
                FatCrabTradeRsp::Accept { receive_address: _ } => {
                    self.data.set_state(FatCrabTakerState::OfferAccepted);
                }
                FatCrabTradeRsp::Reject => {
                    self.data.set_state(FatCrabTakerState::OfferRejected);
                }
            }

            let trade_rsp_notif = FatCrabTakerNotifTradeRspStruct {
                state: self.data.state(),
                trade_rsp_envelope: fatcrab_trade_rsp_envelope,
            };

            notif_tx
                .send(FatCrabTakerNotif::TradeRsp(trade_rsp_notif))
                .await
                .unwrap();
        } else {
            warn!(
                "Taker w/ TradeUUID {} do not have notif_tx registered",
                self.trade_uuid
            );
        }
    }

    fn query_trade_rsp(
        &self,
        rsp_tx: oneshot::Sender<Result<Option<FatCrabTradeRspEnvelope>, FatCrabError>>,
    ) {
        let trade_rsp = self.data.trade_rsp_envelope();
        rsp_tx.send(Ok(trade_rsp)).unwrap();
    }

    async fn notify_peer(
        &self,
        txid: String,
        rsp_tx: oneshot::Sender<Result<FatCrabTakerState, FatCrabError>>,
    ) {
        let message = FatCrabPeerMessage {
            receive_address: self.data.btc_rx_addr().to_string(),
            txid,
        };

        match self.n3xb_taker.send_peer_message(Box::new(message)).await {
            Ok(_) => {
                let new_state = FatCrabTakerState::NotifiedOutbound;
                self.data.set_state(new_state.clone());
                rsp_tx.send(Ok(new_state)).unwrap();
            }
            Err(error) => {
                rsp_tx.send(Err(error.into())).unwrap();
            }
        }
    }

    fn handle_peer_notif(&self, fatcrab_peer_message: &FatCrabPeerMessage) {
        // Should be recieving BTC Txid along with Fatcrab Rx Address here
        // Retain and Notify User
        let txid = match Txid::from_str(&fatcrab_peer_message.txid) {
            Ok(txid) => txid,
            Err(error) => {
                error!(
                    "Taker w/ TradeUUID {} Txid received from Peer {:?} is not valid - {}",
                    self.trade_uuid,
                    fatcrab_peer_message.txid,
                    error.to_string()
                );
                return;
            }
        };

        self.data.set_peer_btc_txid(txid);
    }

    async fn check_btc_tx_confirmation(&self, rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>) {
        let txid = match self.data.peer_btc_txid() {
            Some(txid) => txid,
            None => {
                let error = FatCrabError::Simple {
                    description: format!(
                        "Taker w/ TradeUUID {} should have received BTC Txid from peer",
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

    async fn trade_complete(
        self,
        rsp_tx: oneshot::Sender<Result<FatCrabTakerState, FatCrabError>>,
    ) {
        let new_state = FatCrabTakerState::TradeCompleted;
        self.data.set_state(new_state.clone());
        self.data.terminate();

        match self.n3xb_taker.trade_complete().await {
            Ok(_) => {
                rsp_tx.send(Ok(new_state)).unwrap();
            }
            Err(error) => {
                rsp_tx.send(Err(error.into())).unwrap();
            }
        }
    }
}

struct FatCrabTakerSellActor {
    trade_uuid: Uuid,
    data: FatCrabTakerSellData,
    n3xb_taker: TakerAccess,
    purse: PurseAccess,
    notif_tx: Option<mpsc::Sender<FatCrabTakerNotif>>,
}

impl FatCrabTakerSellActor {
    async fn new(
        order_envelope: &FatCrabOrderEnvelope,
        fatcrab_rx_addr: String,
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        dir_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let sats = order_envelope.order.amount * order_envelope.order.price;
        let funding_result = purse.allocate_funds(sats as u64).await;

        let btc_funds_id = match funding_result {
            Ok(btc_funds_id) => btc_funds_id,
            Err(error) => {
                return Err(error);
            }
        };

        let data = FatCrabTakerSellData::new(
            order_envelope,
            fatcrab_rx_addr.clone(),
            btc_funds_id,
            dir_path,
        );

        let taker_sell_actor = Self {
            trade_uuid: order_envelope.order.trade_uuid,
            data,
            n3xb_taker,
            purse,
            notif_tx: None,
        };

        return Ok(taker_sell_actor);
    }

    fn restore(
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<(Uuid, Self), FatCrabError> {
        let (trade_uuid, data) = FatCrabTakerSellData::restore(data_path)?;
        let actor = Self {
            trade_uuid,
            data,
            n3xb_taker,
            purse,
            notif_tx: None,
        };
        Ok((trade_uuid, actor))
    }

    async fn handle_trade_rsp_notif(
        &mut self,
        trade_rsp: FatCrabTradeRsp,
        n3xb_trade_rsp_envelope: TradeResponseEnvelope,
    ) {
        match trade_rsp.clone() {
            FatCrabTradeRsp::Accept { receive_address } => {
                self.data.set_state(FatCrabTakerState::OfferAccepted);

                // For Maker Sell, Taker Buy Orders
                // There's nothing preventing auto remit pre-allocated funds to Maker
                // Delay notifying User. User will be notified when Maker notifies Taker that Fatcrab got remitted
                let address = Address::from_str(&receive_address).unwrap();
                let btc_addr =
                    match address.require_network(self.purse.network) {
                        Ok(address) => address,
                        Err(error) => {
                            // TODO: Notify user that auto-remit failed?
                            error!(
                            "Taker w/ TradeUUID {} Address received from Peer {:?} is not {} - {}",
                            self.trade_uuid, receive_address, self.purse.network, error.to_string()
                        );
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
                        // TODO: Notify user that auto-remit failed?
                        error!(
                            "Taker w/ TradeUUID {} unable to send funds - {}",
                            self.trade_uuid,
                            error.to_string()
                        );
                        return;
                    }
                };

                let message = FatCrabPeerMessage {
                    receive_address: self.data.fatcrab_rx_addr(),
                    txid: txid.to_string(),
                };

                if let Some(error) = self
                    .n3xb_taker
                    .send_peer_message(Box::new(message))
                    .await
                    .err()
                {
                    // TODO: Notify user that auto-remit notification failed?
                    error!(
                        "Taker w/ TradeUUID {} unable to send Peer Message - {}",
                        self.trade_uuid,
                        error.to_string()
                    );
                    return;
                };

                if let Some(notif_tx) = &self.notif_tx {
                    self.data.set_state(FatCrabTakerState::NotifiedOutbound);

                    // Notify User that Offer was accepted and funds were remitted
                    let fatcrab_trade_rsp_envelope = FatCrabTradeRspEnvelope {
                        _envelope: n3xb_trade_rsp_envelope.clone(),
                        trade_rsp,
                    };
                    let trade_rsp_notif = FatCrabTakerNotifTradeRspStruct {
                        state: self.data.state(),
                        trade_rsp_envelope: fatcrab_trade_rsp_envelope,
                    };
                    notif_tx
                        .send(FatCrabTakerNotif::TradeRsp(trade_rsp_notif))
                        .await
                        .unwrap();
                } else {
                    warn!(
                        "Taker w/ TradeUUID {} do not have notif_tx registered",
                        self.trade_uuid
                    );
                }
            }
            FatCrabTradeRsp::Reject => {
                self.data.set_state(FatCrabTakerState::OfferRejected);

                if let Some(notif_tx) = &self.notif_tx {
                    // Notify User that Offer was rejected
                    let fatcrab_trade_rsp_envelope = FatCrabTradeRspEnvelope {
                        _envelope: n3xb_trade_rsp_envelope.clone(),
                        trade_rsp,
                    };
                    let trade_rsp_notif = FatCrabTakerNotifTradeRspStruct {
                        state: self.data.state(),
                        trade_rsp_envelope: fatcrab_trade_rsp_envelope,
                    };
                    notif_tx
                        .send(FatCrabTakerNotif::TradeRsp(trade_rsp_notif))
                        .await
                        .unwrap();
                } else {
                    warn!(
                        "Taker w/ TradeUUID {} do not have notif_tx registered",
                        self.trade_uuid
                    );
                }
            }
        }
    }

    fn handle_peer_notif(&mut self, _fatcrab_peer_message: &FatCrabPeerMessage) {
        // Nothing to do here
    }

    fn query_trade_rsp(
        &self,
        rsp_tx: oneshot::Sender<Result<Option<FatCrabTradeRspEnvelope>, FatCrabError>>,
    ) {
        rsp_tx
            .send(Err(FatCrabError::Simple {
                description: format!(
                    "Taker w/ TradeUUID {} is a Seller, does not retain Trade Response for query",
                    self.trade_uuid
                ),
            }))
            .unwrap();
    }

    fn notify_peer(
        &self,
        _txid: String,
        rsp_tx: oneshot::Sender<Result<FatCrabTakerState, FatCrabError>>,
    ) {
        rsp_tx
            .send(Err(FatCrabError::Simple {
                description: format!(
                    "Taker w/ TradeUUID {} is a Buyer, does not support manually notifying Peer",
                    self.trade_uuid
                ),
            }))
            .unwrap();
    }

    fn check_btc_tx_confirmation(&self, rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>) {
        rsp_tx
            .send(Err(FatCrabError::Simple {
                description: format!(
                    "Taker w/ TradeUUID {} is a Buyer, will not have a Peer BTC Txid to check on",
                    self.trade_uuid
                ),
            }))
            .unwrap();
    }

    async fn trade_complete(
        self,
        rsp_tx: oneshot::Sender<Result<FatCrabTakerState, FatCrabError>>,
    ) {
        let new_state = FatCrabTakerState::TradeCompleted;
        self.data.set_state(new_state.clone());
        self.data.terminate();

        match self.n3xb_taker.trade_complete().await {
            Ok(_) => {
                rsp_tx.send(Ok(new_state)).unwrap();
            }
            Err(error) => {
                rsp_tx.send(Err(error.into())).unwrap();
            }
        }
    }
}
