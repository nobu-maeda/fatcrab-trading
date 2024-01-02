use std::{marker::PhantomData, path::Path, str::FromStr};

use log::{error, warn};

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

use super::data::{FatCrabTakerBuyData, FatCrabTakerSellData};

pub enum FatCrabTakerNotif {
    TradeRsp(FatCrabTradeRspEnvelope),
    Peer(FatCrabPeerEnvelope),
}

// Just for purpose of typing the Taker
pub struct TakerBuy {} //  Means Taker is taking a Buy Order to Sell
pub struct TakerSell {} // Means Tkaer is taking a Sell Order to Buy

pub enum FatCrabTakerAccessEnum {
    Buy(FatCrabTakerAccess<TakerBuy>),
    Sell(FatCrabTakerAccess<TakerSell>),
}

#[derive(Clone)]
pub struct FatCrabTakerAccess<OrderType = TakerBuy> {
    tx: mpsc::Sender<FatCrabTakerRequest>,
    _order_type: PhantomData<OrderType>,
}

impl FatCrabTakerAccess<TakerBuy> {
    // Means Taker is taking a Buy Order to Sell
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

    pub async fn check_btc_tx_confirmation(&self) -> Result<u32, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<u32, FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::CheckBtcTxConf { rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }
}

impl FatCrabTakerAccess<TakerSell> {}

impl<OrderType> FatCrabTakerAccess<OrderType> {
    pub async fn take_order(&self) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::TakeOrder { rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub async fn trade_complete(&self) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        self.tx
            .send(FatCrabTakerRequest::TradeComplete { rsp_tx })
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
    ) -> Self {
        assert_eq!(order_envelope.order.order_type, FatCrabOrderType::Buy);
        let (tx, rx) =
            mpsc::channel::<FatCrabTakerRequest>(FatCrabTaker::TAKE_TRADE_REQUEST_CHANNEL_SIZE);
        let actor = FatCrabTakerActor::new(
            rx,
            order_envelope.to_owned(),
            None,
            n3xb_taker,
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
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let (tx, rx) =
            mpsc::channel::<FatCrabTakerRequest>(FatCrabTaker::TAKE_TRADE_REQUEST_CHANNEL_SIZE);

        let actor = FatCrabTakerActor::restore_buy_actor(rx, n3xb_taker, purse, data_path).await?;
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
    ) -> Self {
        assert_eq!(order_envelope.order.order_type, FatCrabOrderType::Sell);
        let (tx, rx) =
            mpsc::channel::<FatCrabTakerRequest>(FatCrabTaker::TAKE_TRADE_REQUEST_CHANNEL_SIZE);
        let actor = FatCrabTakerActor::new(
            rx,
            order_envelope.to_owned(),
            Some(fatcrab_rx_addr.into()),
            n3xb_taker,
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
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let (tx, rx) =
            mpsc::channel::<FatCrabTakerRequest>(FatCrabTaker::TAKE_TRADE_REQUEST_CHANNEL_SIZE);

        let actor = FatCrabTakerActor::restore_sell_actor(rx, n3xb_taker, purse, data_path).await?;
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
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    NotifyPeer {
        txid: String,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    CheckBtcTxConf {
        rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>,
    },
    TradeComplete {
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
    ) -> Self {
        let inner = match order_envelope.order.order_type {
            FatCrabOrderType::Buy => {
                let buy_actor =
                    FatCrabTakerBuyActor::new(&order_envelope, n3xb_taker.clone(), purse, dir_path)
                        .await;
                FatCrabTakerInnerActor::Buy(buy_actor)
            }

            FatCrabOrderType::Sell => {
                let sell_actor = FatCrabTakerSellActor::new(
                    &order_envelope,
                    fatcrab_rx_addr.unwrap(),
                    n3xb_taker.clone(),
                    purse,
                    dir_path,
                )
                .await;
                FatCrabTakerInnerActor::Sell(sell_actor)
            }
        };

        Self {
            inner,
            trade_uuid: order_envelope.order.trade_uuid,
            rx,
            notif_tx: None,
            n3xb_taker,
        }
    }

    async fn restore_buy_actor(
        rx: mpsc::Receiver<FatCrabTakerRequest>,
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let (trade_uuid, actor) =
            FatCrabTakerBuyActor::restore(n3xb_taker.clone(), purse, data_path).await?;
        let inner = FatCrabTakerInnerActor::Buy(actor);

        Ok(Self {
            inner,
            trade_uuid,
            rx,
            notif_tx: None,
            n3xb_taker,
        })
    }

    async fn restore_sell_actor(
        rx: mpsc::Receiver<FatCrabTakerRequest>,
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<Self, FatCrabError> {
        let (trade_uuid, actor) =
            FatCrabTakerSellActor::restore(n3xb_taker.clone(), purse, data_path).await?;
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
                        }
                        FatCrabTakerRequest::NotifyPeer { txid, rsp_tx } => {
                            match self.inner {
                                FatCrabTakerInnerActor::Buy(ref buy_actor) => {
                                    buy_actor.notify_peer(txid, rsp_tx).await;
                                }
                                FatCrabTakerInnerActor::Sell(ref sell_actor) => {
                                    sell_actor.notify_peer(txid, rsp_tx).await;
                                }
                            }
                        }
                        FatCrabTakerRequest::CheckBtcTxConf { rsp_tx } => {
                            match self.inner {
                                FatCrabTakerInnerActor::Buy(ref buy_actor) => {
                                    buy_actor.check_btc_tx_confirmation(rsp_tx).await;
                                }
                                FatCrabTakerInnerActor::Sell(ref sell_actor) => {
                                    sell_actor.check_btc_tx_confirmation(rsp_tx).await;
                                }
                            }
                        }
                        FatCrabTakerRequest::TradeComplete { rsp_tx } => {
                            match self.inner {
                                FatCrabTakerInnerActor::Buy(buy_actor) => {
                                    buy_actor.trade_complete(rsp_tx).await;
                                }
                                FatCrabTakerInnerActor::Sell(sell_actor) => {
                                    sell_actor.trade_complete(rsp_tx).await;
                                }
                            }
                            return;
                        }
                        FatCrabTakerRequest::RegisterNotifTx { tx, rsp_tx } => {
                            self.register_notif_tx(tx, rsp_tx).await;
                        }
                        FatCrabTakerRequest::UnregisterNotifTx { rsp_tx } => {
                            self.unregister_notif_tx(rsp_tx).await;
                        }
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

    async fn take_order(&self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        if let Some(error) = self.n3xb_taker.take_order().await.err() {
            rsp_tx.send(Err(error.into())).unwrap();
        } else {
            rsp_tx.send(Ok(())).unwrap();
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

        match self.inner {
            FatCrabTakerInnerActor::Buy(ref mut buy_actor) => {
                buy_actor.handle_peer_notif(&fatcrab_peer_message).await;
            }
            FatCrabTakerInnerActor::Sell(ref mut sell_actor) => {
                sell_actor.handle_peer_notif(&fatcrab_peer_message).await;
            }
        }

        if let Some(notif_tx) = &self.notif_tx {
            let fatcrab_peer_envelope = FatCrabPeerEnvelope {
                _envelope: peer_envelope,
                message: fatcrab_peer_message,
            };
            notif_tx
                .send(FatCrabTakerNotif::Peer(fatcrab_peer_envelope))
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

        let data = FatCrabTakerBuyData::new(
            order_envelope.order.trade_uuid,
            purse.network,
            btc_rx_addr,
            dir_path,
        )
        .await;

        Self {
            trade_uuid: order_envelope.order.trade_uuid,
            data,
            n3xb_taker,
            purse,
            notif_tx: None,
        }
    }

    async fn restore(
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<(Uuid, Self), FatCrabError> {
        let (trade_uuid, data) = FatCrabTakerBuyData::restore(purse.network, data_path).await?;
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
                trade_rsp,
            };
            notif_tx
                .send(FatCrabTakerNotif::TradeRsp(fatcrab_trade_rsp_envelope))
                .await
                .unwrap();
        } else {
            warn!(
                "Taker w/ TradeUUID {} do not have notif_tx registered",
                self.trade_uuid
            );
        }
    }

    async fn notify_peer(&self, txid: String, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        let message = FatCrabPeerMessage {
            receive_address: self.data.btc_rx_addr().await.to_string(),
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

    async fn handle_peer_notif(&self, fatcrab_peer_message: &FatCrabPeerMessage) {
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

        self.data.set_peer_btc_txid(txid).await;
    }

    async fn check_btc_tx_confirmation(&self, rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>) {
        let txid = match self.data.peer_btc_txid().await {
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

    async fn trade_complete(self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        self.data.set_trade_completed().await;
        self.data.terminate().await;

        match self.n3xb_taker.trade_complete().await {
            Ok(_) => {
                rsp_tx.send(Ok(())).unwrap();
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
    ) -> Self {
        let sats = order_envelope.order.amount * order_envelope.order.price;
        let btc_funds_id = purse.allocate_funds(sats as u64).await.unwrap();

        let data = FatCrabTakerSellData::new(
            order_envelope.order.trade_uuid,
            fatcrab_rx_addr.clone(),
            btc_funds_id,
            dir_path,
        )
        .await;

        Self {
            trade_uuid: order_envelope.order.trade_uuid,
            data,
            n3xb_taker,
            purse,
            notif_tx: None,
        }
    }

    async fn restore(
        n3xb_taker: TakerAccess,
        purse: PurseAccess,
        data_path: impl AsRef<Path>,
    ) -> Result<(Uuid, Self), FatCrabError> {
        let (trade_uuid, data) = FatCrabTakerSellData::restore(data_path).await?;
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
                // For Maker Sell, Taker Buy Orders
                // There's nothing preventing auto remit pre-allocated funds to Maker
                // Delay notifying User. User will be notified when Maker notifies Taker that Fatcrab got remitted
                let address = Address::from_str(&receive_address).unwrap();
                let btc_addr =
                    match address.require_network(self.purse.network) {
                        Ok(address) => address,
                        Err(error) => {
                            error!(
                            "Taker w/ TradeUUID {} Address received from Peer {:?} is not {} - {}",
                            self.trade_uuid, receive_address, self.purse.network, error.to_string()
                        );
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
                        error!(
                            "Taker w/ TradeUUID {} unable to send funds - {}",
                            self.trade_uuid,
                            error.to_string()
                        );
                        return;
                    }
                };

                let message = FatCrabPeerMessage {
                    receive_address: self.data.fatcrab_rx_addr().await,
                    txid: txid.to_string(),
                };

                if let Some(error) = self
                    .n3xb_taker
                    .send_peer_message(Box::new(message))
                    .await
                    .err()
                {
                    error!(
                        "Taker w/ TradeUUID {} unable to send Peer Message - {}",
                        self.trade_uuid,
                        error.to_string()
                    );
                }
            }
            FatCrabTradeRsp::Reject => {
                if let Some(notif_tx) = &self.notif_tx {
                    // Notify User that Offer was rejected
                    let fatcrab_trade_rsp_envelope = FatCrabTradeRspEnvelope {
                        _envelope: n3xb_trade_rsp_envelope.clone(),
                        trade_rsp,
                    };
                    notif_tx
                        .send(FatCrabTakerNotif::TradeRsp(fatcrab_trade_rsp_envelope))
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

    async fn handle_peer_notif(&mut self, _fatcrab_peer_message: &FatCrabPeerMessage) {
        // Nothing to do here
    }

    async fn notify_peer(&self, _txid: String, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        rsp_tx
            .send(Err(FatCrabError::Simple {
                description: format!(
                    "Taker w/ TradeUUID {} is a Buyer, does not support manually notifying Peer",
                    self.trade_uuid
                ),
            }))
            .unwrap();
    }

    async fn check_btc_tx_confirmation(&self, rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>) {
        rsp_tx
            .send(Err(FatCrabError::Simple {
                description: format!(
                    "Taker w/ TradeUUID {} is a Buyer, will not have a Peer BTC Txid to check on",
                    self.trade_uuid
                ),
            }))
            .unwrap();
    }

    async fn trade_complete(self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        self.data.set_trade_completed().await;
        self.data.terminate().await;

        match self.n3xb_taker.trade_complete().await {
            Ok(_) => {
                rsp_tx.send(Ok(())).unwrap();
            }
            Err(error) => {
                rsp_tx.send(Err(error.into())).unwrap();
            }
        }
    }
}
