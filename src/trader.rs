use log::{debug, warn};
use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    path::Path,
};
use url::Url;

use bip39::Mnemonic;
use bitcoin::{Address, Txid};
use crusty_n3xb::{
    common::types::{BitcoinSettlementMethod, ObligationKind},
    maker::MakerAccess,
    manager::Manager,
    order::FilterTag,
    taker::TakerAccess,
};
use secp256k1::{rand, SecretKey, XOnlyPublicKey};
use tokio::sync::RwLock;
use tokio::task::JoinError;
use uuid::Uuid;

use crate::{
    common::{BlockchainInfo, FATCRAB_OBLIGATION_CUSTOM_KIND_STRING},
    error::FatCrabError,
    maker::{
        FatCrabMaker, FatCrabMakerAccess, FatCrabMakerAccessEnum, FatCrabMakerEnum, MakerBuy,
        MakerSell,
    },
    offer::FatCrabOffer,
    order::{FatCrabOrder, FatCrabOrderEnvelope, FatCrabOrderType},
    purse::{Purse, PurseAccess},
    taker::{
        FatCrabTaker, FatCrabTakerAccess, FatCrabTakerAccessEnum, FatCrabTakerEnum, TakerBuy,
        TakerSell,
    },
};

pub struct FatCrabTrader {
    n3xb_manager: Manager,
    purse: Purse,
    purse_accessor: PurseAccess,
    makers: RwLock<HashMap<Uuid, FatCrabMakerEnum>>,
    takers: RwLock<HashMap<Uuid, FatCrabTakerEnum>>,
    maker_accessors: RwLock<HashMap<Uuid, FatCrabMakerAccessEnum>>,
    taker_accessors: RwLock<HashMap<Uuid, FatCrabTakerAccessEnum>>,
}

const DATA_DIR_PATH_STR: &str = "fatcrab_data";

impl FatCrabTrader {
    pub async fn new(info: BlockchainInfo) -> Self {
        let secret_key = SecretKey::new(&mut rand::thread_rng());
        Self::new_with_key(secret_key, info).await
    }

    pub async fn new_with_key(secret_key: SecretKey, info: BlockchainInfo) -> Self {
        let trade_engine_name = "fat-crab-trade-engine";
        let n3xb_manager = Manager::new_with_key(secret_key, trade_engine_name).await;
        let purse = Purse::new(secret_key, info);
        let purse_accessor = purse.new_accessor();

        let n3xb_makers = n3xb_manager.get_makers().await;
        let n3xb_takers = n3xb_manager.get_takers().await;
        let pubkey = n3xb_manager.pubkey().await;

        let (makers, takers) = Self::maker_taker_setup_restore(
            &n3xb_makers,
            &n3xb_takers,
            &purse_accessor,
            pubkey.to_string(),
        )
        .await;

        Self {
            n3xb_manager,
            purse,
            purse_accessor,
            makers: RwLock::new(makers),
            takers: RwLock::new(takers),
            maker_accessors: RwLock::new(HashMap::new()),
            taker_accessors: RwLock::new(HashMap::new()),
        }
    }

    fn maker_buy_data_dir_path(identifier: impl AsRef<str>) -> String {
        format!("{}/{}/makers/buy/", DATA_DIR_PATH_STR, identifier.as_ref())
    }

    fn maker_sell_data_dir_path(identifier: impl AsRef<str>) -> String {
        format!("{}/{}/makers/sell/", DATA_DIR_PATH_STR, identifier.as_ref())
    }

    fn taker_buy_data_dir_path(identifier: impl AsRef<str>) -> String {
        format!("{}/{}/takers/buy/", DATA_DIR_PATH_STR, identifier.as_ref())
    }

    fn taker_sell_data_dir_path(identifier: impl AsRef<str>) -> String {
        format!("{}/{}/takers/sell/", DATA_DIR_PATH_STR, identifier.as_ref())
    }

    async fn maker_taker_setup_restore(
        n3xb_makers: &HashMap<Uuid, MakerAccess>,
        n3xb_takers: &HashMap<Uuid, TakerAccess>,
        purse_accessor: &PurseAccess,
        pubkey_string: impl AsRef<str>,
    ) -> (
        HashMap<Uuid, FatCrabMakerEnum>,
        HashMap<Uuid, FatCrabTakerEnum>,
    ) {
        let result: Result<
            (
                HashMap<Uuid, FatCrabMakerEnum>,
                HashMap<Uuid, FatCrabTakerEnum>,
            ),
            FatCrabError,
        > = async {
            let maker_buy_dir_path = Self::maker_buy_data_dir_path(&pubkey_string);
            tokio::fs::create_dir_all(&maker_buy_dir_path).await?;
            let buy_makers =
                Self::restore_buy_makers(n3xb_makers, purse_accessor, &maker_buy_dir_path).await?;

            let maker_sell_dir_path = Self::maker_sell_data_dir_path(&pubkey_string);
            tokio::fs::create_dir_all(&maker_sell_dir_path).await?;
            let sell_makers =
                Self::restore_sell_makers(n3xb_makers, purse_accessor, &maker_sell_dir_path)
                    .await?;

            // let taker_buy_dir_path = Self::taker_buy_data_dir_path(&pubkey_string);
            // tokio::fs::create_dir_all(&taker_buy_dir_path).await?;
            // let buy_takers =
            //     Self::restore_buy_takers(n3xb_takers, purse_accessor, &taker_buy_dir_path).await?;

            // let taker_sell_dir_path = Self::taker_sell_data_dir_path(&pubkey_string);
            // tokio::fs::create_dir_all(&taker_sell_dir_path).await?;
            // let sell_takers =
            //     Self::restore_sell_takers(n3xb_takers, purse_accessor, &taker_sell_dir_path)
            //         .await?;

            let makers = buy_makers
                .into_iter()
                .chain(sell_makers.into_iter())
                .collect();

            // let takers = buy_takers
            //     .into_iter()
            //     .chain(sell_takers.into_iter())
            //     .collect();

            Ok((makers, HashMap::new()))
        }
        .await;

        match result {
            Ok((makers, takers)) => {
                debug!(
                    "Trader w/ pubkey {} restored {} Makers and {} Takers",
                    pubkey_string.as_ref(),
                    makers.len(),
                    takers.len()
                );
                (makers, takers)
            }
            Err(err) => {
                warn!("Error setting up & restoring from data directory - {}", err);
                (HashMap::new(), HashMap::new())
            }
        }
    }

    async fn restore_buy_makers(
        n3xb_makers: &HashMap<Uuid, MakerAccess>,
        purse_accessor: &PurseAccess,
        maker_buy_dir_path: impl AsRef<Path>,
    ) -> Result<HashMap<Uuid, FatCrabMakerEnum>, FatCrabError> {
        let mut makers = HashMap::new();
        let mut maker_files = tokio::fs::read_dir(maker_buy_dir_path.as_ref()).await?;

        while let Some(maker_file) = maker_files.next_entry().await.unwrap() {
            if let Some((trade_uuid, maker)) =
                Self::restore_buy_maker(n3xb_makers, purse_accessor, maker_file.path()).await
            {
                makers.insert(trade_uuid, maker);
            }
        }
        Ok(makers)
    }

    async fn restore_buy_maker(
        n3xb_makers: &HashMap<Uuid, MakerAccess>,
        purse_accessor: &PurseAccess,
        maker_buy_data_path: impl AsRef<Path>,
    ) -> Option<(Uuid, FatCrabMakerEnum)> {
        let file_stem = match maker_buy_data_path.as_ref().file_stem() {
            Some(stem) => stem,
            None => return None,
        };

        let trade_uuid_str = match file_stem.to_str() {
            Some(stem_str) => stem_str,
            None => return None,
        };

        let trade_uuid = match Uuid::parse_str(trade_uuid_str) {
            Ok(uuid) => uuid,
            Err(_) => return None,
        };

        let n3xb_maker = match n3xb_makers.get(&trade_uuid) {
            Some(maker) => maker,
            None => return None,
        };

        let maker = match FatCrabMaker::<MakerBuy>::restore(
            n3xb_maker.clone(),
            purse_accessor.clone(),
            maker_buy_data_path,
        )
        .await
        {
            Ok(maker) => maker,
            Err(_) => return None,
        };
        Some((trade_uuid, FatCrabMakerEnum::Buy(maker)))
    }

    async fn restore_sell_makers(
        n3xb_makers: &HashMap<Uuid, MakerAccess>,
        purse_accessor: &PurseAccess,
        maker_sell_dir_path: impl AsRef<Path>,
    ) -> Result<HashMap<Uuid, FatCrabMakerEnum>, FatCrabError> {
        let mut makers = HashMap::new();
        let mut maker_files = tokio::fs::read_dir(maker_sell_dir_path.as_ref()).await?;

        while let Some(maker_file) = maker_files.next_entry().await.unwrap() {
            if let Some((trade_uuid, maker)) =
                Self::restore_sell_maker(n3xb_makers, purse_accessor, maker_file.path()).await
            {
                makers.insert(trade_uuid, maker);
            }
        }
        Ok(makers)
    }

    async fn restore_sell_maker(
        n3xb_makers: &HashMap<Uuid, MakerAccess>,
        purse_accessor: &PurseAccess,
        maker_sell_data_path: impl AsRef<Path>,
    ) -> Option<(Uuid, FatCrabMakerEnum)> {
        let file_stem = match maker_sell_data_path.as_ref().file_stem() {
            Some(stem) => stem,
            None => return None,
        };

        let trade_uuid_str = match file_stem.to_str() {
            Some(stem_str) => stem_str,
            None => return None,
        };

        let trade_uuid = match Uuid::parse_str(trade_uuid_str) {
            Ok(uuid) => uuid,
            Err(_) => return None,
        };

        let n3xb_maker = match n3xb_makers.get(&trade_uuid) {
            Some(maker) => maker,
            None => return None,
        };

        let maker = match FatCrabMaker::<MakerSell>::restore(
            n3xb_maker.clone(),
            purse_accessor.clone(),
            maker_sell_data_path,
        )
        .await
        {
            Ok(maker) => maker,
            Err(_) => return None,
        };
        Some((trade_uuid, FatCrabMakerEnum::Sell(maker)))
    }

    // async fn restore_buy_takers(
    //     n3xb_takers: &HashMap<Uuid, TakerAccess>,
    //     purse_accessor: &PurseAccess,
    //     taker_buy_dir_path: impl AsRef<Path>,
    // ) -> Result<HashMap<Uuid, FatCrabTakerEnum>, FatCrabError> {
    //     let mut takers = HashMap::new();
    //     let mut taker_files = tokio::fs::read_dir(taker_buy_dir_path.as_ref()).await?;

    //     while let Some(taker_file) = taker_files.next_entry().await.unwrap() {
    //         if let Some((trade_uuid, taker)) =
    //             Self::restore_buy_taker(n3xb_takers, purse_accessor, taker_file.path()).await
    //         {
    //             takers.insert(trade_uuid, taker);
    //         }
    //     }
    //     Ok(takers)
    // }

    // async fn restore_buy_taker(
    //     n3xb_takers: &HashMap<Uuid, TakerAccess>,
    //     purse_accessor: &PurseAccess,
    //     taker_buy_data_path: impl AsRef<Path>,
    // ) -> Option<(Uuid, FatCrabTakerEnum)> {
    //     let file_stem = match taker_buy_data_path.as_ref().file_stem() {
    //         Some(stem) => stem,
    //         None => return None,
    //     };

    //     let trade_uuid_str = match file_stem.to_str() {
    //         Some(stem_str) => stem_str,
    //         None => return None,
    //     };

    //     let trade_uuid = match Uuid::parse_str(trade_uuid_str) {
    //         Ok(uuid) => uuid,
    //         Err(_) => return None,
    //     };

    //     let n3xb_taker = match n3xb_takers.get(&trade_uuid) {
    //         Some(taker) => taker,
    //         None => return None,
    //     };

    //     let taker = match FatCrabTaker::<TakerBuy>::restore(
    //         n3xb_taker.clone(),
    //         purse_accessor.clone(),
    //         taker_buy_data_path,
    //     )
    //     .await
    //     {
    //         Ok(taker) => taker,
    //         Err(_) => return None,
    //     };
    //     Some((trade_uuid, FatCrabTakerEnum::Buy(taker)))
    // }

    // async fn restore_sell_takers(
    //     n3xb_takers: &HashMap<Uuid, TakerAccess>,
    //     purse_accessor: &PurseAccess,
    //     taker_sell_dir_path: impl AsRef<Path>,
    // ) -> Result<HashMap<Uuid, FatCrabTakerEnum>, FatCrabError> {
    //     let mut takers = HashMap::new();
    //     let mut taker_files = tokio::fs::read_dir(taker_sell_dir_path.as_ref()).await?;

    //     while let Some(taker_file) = taker_files.next_entry().await.unwrap() {
    //         if let Some((trade_uuid, taker)) =
    //             Self::restore_sell_taker(n3xb_takers, purse_accessor, taker_file.path()).await
    //         {
    //             takers.insert(trade_uuid, taker);
    //         }
    //     }
    //     Ok(takers)
    // }

    // async fn restore_sell_taker(
    //     n3xb_takers: &HashMap<Uuid, TakerAccess>,
    //     purse_accessor: &PurseAccess,
    //     taker_sell_data_path: impl AsRef<Path>,
    // ) -> Option<(Uuid, FatCrabTakerEnum)> {
    //     let file_stem = match taker_sell_data_path.as_ref().file_stem() {
    //         Some(stem) => stem,
    //         None => return None,
    //     };

    //     let trade_uuid_str = match file_stem.to_str() {
    //         Some(stem_str) => stem_str,
    //         None => return None,
    //     };

    //     let trade_uuid = match Uuid::parse_str(trade_uuid_str) {
    //         Ok(uuid) => uuid,
    //         Err(_) => return None,
    //     };

    //     let n3xb_taker = match n3xb_takers.get(&trade_uuid) {
    //         Some(taker) => taker,
    //         None => return None,
    //     };

    //     let taker = match FatCrabTaker::<TakerSell>::restore(
    //         n3xb_taker.clone(),
    //         purse_accessor.clone(),
    //         taker_sell_data_path,
    //     )
    //     .await
    //     {
    //         Ok(taker) => taker,
    //         Err(_) => return None,
    //     };
    //     Some((trade_uuid, FatCrabTakerEnum::Sell(taker)))
    // }

    pub async fn wallet_bip39_mnemonic(&self) -> Result<Mnemonic, FatCrabError> {
        self.purse_accessor.get_mnemonic().await
    }

    pub async fn wallet_spendable_balance(&self) -> Result<u64, FatCrabError> {
        self.purse_accessor.get_spendable_balance().await
    }

    pub async fn wallet_generate_receive_address(&self) -> Result<Address, FatCrabError> {
        self.purse_accessor.get_rx_address().await
    }

    pub async fn wallet_send_to_address(
        &self,
        address: Address,
        sats: u64,
    ) -> Result<Txid, FatCrabError> {
        let funds_id = self.purse_accessor.allocate_funds(sats).await?;
        self.purse_accessor.send_funds(funds_id, address).await
    }

    pub async fn wallet_blockchain_sync(&self) -> Result<(), FatCrabError> {
        self.purse_accessor.sync_blockchain().await
    }

    pub async fn nostr_pubkey(&self) -> XOnlyPublicKey {
        self.n3xb_manager.pubkey().await
    }

    pub async fn add_relays(
        &self,
        relays: Vec<(Url, Option<SocketAddr>)>,
    ) -> Result<(), FatCrabError> {
        self.n3xb_manager.add_relays(relays, true).await?;
        Ok(())
    }

    pub async fn make_buy_order(
        &self,
        order: FatCrabOrder,
        fatcrab_rx_addr: impl Into<String>,
    ) -> FatCrabMakerAccess<MakerBuy> {
        assert_eq!(order.order_type, FatCrabOrderType::Buy);

        let n3xb_maker = self.n3xb_manager.new_maker(order.clone().into()).await;
        let purse_accessor = self.purse.new_accessor();
        let pubkey = self.n3xb_manager.pubkey().await;
        let trade_uuid = order.trade_uuid.clone();

        let maker = FatCrabMaker::<MakerBuy>::new(
            order,
            fatcrab_rx_addr,
            n3xb_maker,
            purse_accessor,
            Self::maker_buy_data_dir_path(pubkey.to_string()),
        )
        .await;
        let maker_accessor = maker.new_accessor();
        let maker_return_accessor = maker.new_accessor();

        let mut makers = self.makers.write().await;
        makers.insert(trade_uuid.clone(), FatCrabMakerEnum::Buy(maker));

        let mut maker_accessors = self.maker_accessors.write().await;
        maker_accessors.insert(trade_uuid, FatCrabMakerAccessEnum::Buy(maker_accessor));

        maker_return_accessor
    }

    pub async fn make_sell_order(&self, order: FatCrabOrder) -> FatCrabMakerAccess<MakerSell> {
        assert_eq!(order.order_type, FatCrabOrderType::Sell);

        let n3xb_maker = self.n3xb_manager.new_maker(order.clone().into()).await;
        let purse_accessor = self.purse.new_accessor();
        let pubkey = self.n3xb_manager.pubkey().await;
        let trade_uuid = order.trade_uuid.clone();

        let maker = FatCrabMaker::<MakerSell>::new(
            order,
            n3xb_maker,
            purse_accessor,
            Self::maker_sell_data_dir_path(pubkey.to_string()),
        )
        .await;
        let maker_accessor = maker.new_accessor();
        let maker_return_accessor = maker.new_accessor();

        let mut makers = self.makers.write().await;
        makers.insert(trade_uuid.clone(), FatCrabMakerEnum::Sell(maker));

        let mut maker_accessors = self.maker_accessors.write().await;
        maker_accessors.insert(trade_uuid, FatCrabMakerAccessEnum::Sell(maker_accessor));

        maker_return_accessor
    }

    pub async fn query_orders(
        &self,
        order_type: FatCrabOrderType,
    ) -> Result<Vec<FatCrabOrderEnvelope>, FatCrabError> {
        let custom_fatcrab_obligation_kind: ObligationKind =
            ObligationKind::Custom(FATCRAB_OBLIGATION_CUSTOM_KIND_STRING.to_string());
        let bitcoin_onchain_obligation_kind: ObligationKind =
            ObligationKind::Bitcoin(Some(BitcoinSettlementMethod::Onchain));

        let mut filter_tags = Vec::new();
        match order_type {
            FatCrabOrderType::Buy => {
                let maker_obligation_filter = FilterTag::MakerObligations(HashSet::from_iter(
                    vec![bitcoin_onchain_obligation_kind.clone()].into_iter(),
                ));
                let taker_obligation_filter = FilterTag::TakerObligations(HashSet::from_iter(
                    vec![custom_fatcrab_obligation_kind.clone()].into_iter(),
                ));
                filter_tags.push(maker_obligation_filter);
                filter_tags.push(taker_obligation_filter);
            }

            FatCrabOrderType::Sell => {
                let maker_obligation_filter = FilterTag::MakerObligations(HashSet::from_iter(
                    vec![custom_fatcrab_obligation_kind.clone()].into_iter(),
                ));
                let taker_obligation_filter = FilterTag::TakerObligations(HashSet::from_iter(
                    vec![bitcoin_onchain_obligation_kind.clone()].into_iter(),
                ));
                filter_tags.push(maker_obligation_filter);
                filter_tags.push(taker_obligation_filter);
            }
        }

        let n3xb_orders = self.n3xb_manager.query_orders(filter_tags).await?;
        let orders: Vec<FatCrabOrderEnvelope> = n3xb_orders
            .into_iter()
            .map(|envelope| FatCrabOrderEnvelope {
                order: FatCrabOrder::from_n3xb_order(envelope.order.clone()).unwrap(),
                envelope,
            })
            .collect();
        Ok(orders)
    }

    pub async fn take_buy_order(
        &self,
        order_envelope: FatCrabOrderEnvelope,
    ) -> FatCrabTakerAccess<TakerBuy> {
        assert_eq!(order_envelope.order.order_type, FatCrabOrderType::Buy);

        let n3xb_taker = self
            .n3xb_manager
            .new_taker(
                order_envelope.envelope.clone(),
                FatCrabOffer::create_n3xb_offer(order_envelope.order.clone()),
            )
            .await
            .unwrap();
        n3xb_taker.take_order().await.unwrap();
        let trade_uuid = order_envelope.order.trade_uuid.clone();

        let taker =
            FatCrabTaker::<TakerBuy>::new(order_envelope, n3xb_taker, self.purse.new_accessor())
                .await;
        let taker_accessor = taker.new_accessor();
        let taker_return_accessor = taker.new_accessor();

        let mut takers = self.takers.write().await;
        takers.insert(trade_uuid.clone(), FatCrabTakerEnum::Buy(taker));

        let mut taker_accessors = self.taker_accessors.write().await;
        taker_accessors.insert(trade_uuid, FatCrabTakerAccessEnum::Buy(taker_accessor));

        taker_return_accessor
    }

    pub async fn take_sell_order(
        &self,
        order_envelope: FatCrabOrderEnvelope,
        fatcrab_rx_addr: impl Into<String>,
    ) -> FatCrabTakerAccess<TakerSell> {
        assert_eq!(order_envelope.order.order_type, FatCrabOrderType::Sell);

        let n3xb_taker = self
            .n3xb_manager
            .new_taker(
                order_envelope.envelope.clone(),
                FatCrabOffer::create_n3xb_offer(order_envelope.order.clone()),
            )
            .await
            .unwrap();
        n3xb_taker.take_order().await.unwrap();

        let trade_uuid = order_envelope.order.trade_uuid.clone();

        let taker = FatCrabTaker::<TakerSell>::new(
            order_envelope,
            fatcrab_rx_addr,
            n3xb_taker,
            self.purse.new_accessor(),
        )
        .await;
        let taker_accessor = taker.new_accessor();
        let taker_return_accessor = taker.new_accessor();

        let mut takers = self.takers.write().await;
        takers.insert(trade_uuid.clone(), FatCrabTakerEnum::Sell(taker));

        let mut taker_accessors = self.taker_accessors.write().await;
        taker_accessors.insert(trade_uuid, FatCrabTakerAccessEnum::Sell(taker_accessor));

        taker_return_accessor
    }

    pub async fn shutdown(self) -> Result<(), JoinError> {
        if let Some(error) = self.purse_accessor.shutdown().await.err() {
            warn!("Trader error shutting down Purse: {}", error);
        }
        self.purse.task_handle.await?;
        let mut makers = self.makers.write().await;
        for (_uuid, maker) in makers.drain() {
            maker.await_task_handle().await?;
        }
        let mut takers = self.takers.write().await;
        for (_uuid, taker) in takers.drain() {
            taker.await_task_handle().await?;
        }
        Ok(())
    }
}
