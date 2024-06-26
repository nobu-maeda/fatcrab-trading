use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    path::{Path, PathBuf},
};
use tracing::{debug, warn};
use url::Url;

use bip39::Mnemonic;
use bitcoin::{Address, Txid};

use crusty_n3xb::{
    common::types::{BitcoinNetwork, BitcoinSettlementMethod, ObligationKind},
    maker::MakerAccess,
    manager::Manager,
    order::FilterTag,
    taker::TakerAccess,
};
pub use crusty_n3xb::{RelayInfo, RelayInformationDocument, RelayStatus};

use secp256k1::{rand, SecretKey, XOnlyPublicKey};
use tokio::sync::RwLock;
use tokio::task::JoinError;
use uuid::Uuid;

use crate::{
    common::{Balances, BlockchainInfo, ProductionLevel, FATCRAB_OBLIGATION_CUSTOM_KIND_STRING},
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
    network: bitcoin::Network,
    trader_dir_path: PathBuf,
    purse: Purse,
    purse_accessor: PurseAccess,
    makers: RwLock<HashMap<Uuid, FatCrabMakerEnum>>,
    takers: RwLock<HashMap<Uuid, FatCrabTakerEnum>>,
    maker_accessors: RwLock<HashMap<Uuid, FatCrabMakerAccessEnum>>,
    taker_accessors: RwLock<HashMap<Uuid, FatCrabTakerAccessEnum>>,
}

const TRADE_ENGINE_NAME_DBG_STR: &str = "fatcrab-trade-engine-debug";
const TRADE_ENGINE_NAME_STR: &str = "fatcrab-trade-engine";

const DATA_DIR_PATH_STR: &str = "fatcrab_data";
const PURSE_DIR_STR: &str = "purse";
const MAKER_BUY_DIR_STR: &str = "makers/buy";
const MAKER_SELL_DIR_STR: &str = "makers/sell";
const TAKER_BUY_DIR_STR: &str = "takers/buy";
const TAKER_SELL_DIR_STR: &str = "takers/sell";

impl FatCrabTrader {
    pub async fn new(
        prod_lvl: ProductionLevel,
        info: BlockchainInfo,
        root_dir_path: impl AsRef<Path>,
    ) -> Self {
        let secret_key = SecretKey::new(&mut rand::thread_rng());
        Self::new_with_key(prod_lvl, secret_key, info, root_dir_path).await
    }

    fn bitcoin_network_to_n3xb(network: bitcoin::Network) -> BitcoinNetwork {
        match network {
            bitcoin::Network::Bitcoin => BitcoinNetwork::Mainnet,
            bitcoin::Network::Testnet => BitcoinNetwork::Testnet,
            bitcoin::Network::Regtest => BitcoinNetwork::Regtest,
            bitcoin::Network::Signet => BitcoinNetwork::Signet,
            _ => panic!("FatCrabTrader::bitcoin_network_to_n3xb() - Unsupported Bitcoin Network"),
        }
    }

    fn n3xb_network(&self) -> BitcoinNetwork {
        Self::bitcoin_network_to_n3xb(self.network)
    }

    pub async fn new_with_key(
        prod_lvl: ProductionLevel,
        secret_key: SecretKey,
        info: BlockchainInfo,
        root_dir_path: impl AsRef<Path>,
    ) -> Self {
        let trade_engine_name = match prod_lvl {
            ProductionLevel::Production => TRADE_ENGINE_NAME_STR,
            ProductionLevel::Debug => TRADE_ENGINE_NAME_DBG_STR,
        };
        let network = match info {
            BlockchainInfo::Electrum { network, .. } => network,
            BlockchainInfo::Rpc { network, .. } => network,
        };
        let n3xb_network = Self::bitcoin_network_to_n3xb(network);
        let n3xb_manager =
            Manager::new_with_key(secret_key, trade_engine_name, n3xb_network, &root_dir_path)
                .await;
        let pubkey = n3xb_manager.pubkey().await;

        let trader_dir_path = root_dir_path.as_ref().join(format!(
            "{}/{}/{}/{}",
            DATA_DIR_PATH_STR,
            pubkey.to_string(),
            trade_engine_name,
            network
        ));

        let purse_dir_path = trader_dir_path.join(PURSE_DIR_STR);
        std::fs::create_dir_all(&purse_dir_path).unwrap();

        let purse = Purse::new(secret_key, info, purse_dir_path);
        let purse_accessor = purse.new_accessor();

        let trader = Self {
            n3xb_manager,
            network,
            trader_dir_path,
            purse,
            purse_accessor,
            makers: RwLock::new(HashMap::new()),
            takers: RwLock::new(HashMap::new()),
            maker_accessors: RwLock::new(HashMap::new()),
            taker_accessors: RwLock::new(HashMap::new()),
        };

        trader.restore().await.unwrap();
        trader
    }

    async fn restore(&self) -> Result<(), FatCrabError> {
        // In a restore scenario, We really need to make sure notifications are subscirbed first.
        // Otherwise missing notificaitons will immeidately be notified, which goes nowhere if
        // notifications by the application layer is not already subscribed
        // self.n3xb_manager.connect_all_relays().await?;

        let n3xb_makers = self.n3xb_manager.get_makers().await;
        let n3xb_takers = self.n3xb_manager.get_takers().await;
        let pubkey = self.n3xb_manager.pubkey().await;

        let (makers, takers) = Self::restore_maker_takers(
            &n3xb_makers,
            &n3xb_takers,
            &self.purse_accessor,
            pubkey.to_string(),
            &self.trader_dir_path,
        );

        let maker_accessors: HashMap<Uuid, FatCrabMakerAccessEnum> = makers
            .iter()
            .map(|(trade_uuid, maker)| {
                let maker_accessor = match maker {
                    FatCrabMakerEnum::Buy(maker) => {
                        FatCrabMakerAccessEnum::Buy(maker.new_accessor())
                    }
                    FatCrabMakerEnum::Sell(maker) => {
                        FatCrabMakerAccessEnum::Sell(maker.new_accessor())
                    }
                };
                (trade_uuid.clone(), maker_accessor)
            })
            .collect();

        let taker_accessors: HashMap<Uuid, FatCrabTakerAccessEnum> = takers
            .iter()
            .map(|(trade_uuid, taker)| {
                let taker_accessor = match taker {
                    FatCrabTakerEnum::Buy(taker) => {
                        FatCrabTakerAccessEnum::Buy(taker.new_accessor())
                    }
                    FatCrabTakerEnum::Sell(taker) => {
                        FatCrabTakerAccessEnum::Sell(taker.new_accessor())
                    }
                };
                (trade_uuid.clone(), taker_accessor)
            })
            .collect();

        self.makers.write().await.extend(makers);
        self.takers.write().await.extend(takers);
        self.maker_accessors.write().await.extend(maker_accessors);
        self.taker_accessors.write().await.extend(taker_accessors);

        Ok(())
    }

    fn restore_maker_takers(
        n3xb_makers: &HashMap<Uuid, MakerAccess>,
        n3xb_takers: &HashMap<Uuid, TakerAccess>,
        purse_accessor: &PurseAccess,
        pubkey_string: impl AsRef<str>,
        trader_dir_path: impl AsRef<Path>,
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
        > = (|| {
            let maker_buy_dir_path = trader_dir_path.as_ref().join(MAKER_BUY_DIR_STR);
            std::fs::create_dir_all(&maker_buy_dir_path)?;
            let buy_makers =
                Self::restore_buy_makers(n3xb_makers, purse_accessor, &maker_buy_dir_path)?;

            let maker_sell_dir_path = trader_dir_path.as_ref().join(MAKER_SELL_DIR_STR);
            std::fs::create_dir_all(&maker_sell_dir_path)?;
            let sell_makers =
                Self::restore_sell_makers(n3xb_makers, purse_accessor, &maker_sell_dir_path)?;

            let taker_buy_dir_path = trader_dir_path.as_ref().join(TAKER_BUY_DIR_STR);
            std::fs::create_dir_all(&taker_buy_dir_path)?;
            let buy_takers =
                Self::restore_buy_takers(n3xb_takers, purse_accessor, &taker_buy_dir_path)?;

            let taker_sell_dir_path = trader_dir_path.as_ref().join(TAKER_SELL_DIR_STR);
            std::fs::create_dir_all(&taker_sell_dir_path)?;
            let sell_takers =
                Self::restore_sell_takers(n3xb_takers, purse_accessor, &taker_sell_dir_path)?;

            let makers = buy_makers
                .into_iter()
                .chain(sell_makers.into_iter())
                .collect();

            let takers = buy_takers
                .into_iter()
                .chain(sell_takers.into_iter())
                .collect();

            Ok((makers, takers))
        })();

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

    fn restore_buy_makers(
        n3xb_makers: &HashMap<Uuid, MakerAccess>,
        purse_accessor: &PurseAccess,
        maker_buy_dir_path: impl AsRef<Path>,
    ) -> Result<HashMap<Uuid, FatCrabMakerEnum>, FatCrabError> {
        let mut makers = HashMap::new();
        let mut maker_files = std::fs::read_dir(maker_buy_dir_path.as_ref())?;

        while let Some(maker_file_result) = maker_files.next() {
            if let Some((trade_uuid, maker)) =
                Self::restore_buy_maker(n3xb_makers, purse_accessor, maker_file_result?.path())
            {
                makers.insert(trade_uuid, maker);
            }
        }
        Ok(makers)
    }

    fn restore_buy_maker(
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
        ) {
            Ok(maker) => maker,
            Err(error) => {
                panic!("Error restoring Buy Maker - {}", error);
                // return None;
            }
        };
        Some((trade_uuid, FatCrabMakerEnum::Buy(maker)))
    }

    fn restore_sell_makers(
        n3xb_makers: &HashMap<Uuid, MakerAccess>,
        purse_accessor: &PurseAccess,
        maker_sell_dir_path: impl AsRef<Path>,
    ) -> Result<HashMap<Uuid, FatCrabMakerEnum>, FatCrabError> {
        let mut makers = HashMap::new();
        let mut maker_files = std::fs::read_dir(maker_sell_dir_path.as_ref())?;

        while let Some(maker_file_result) = maker_files.next() {
            if let Some((trade_uuid, maker)) =
                Self::restore_sell_maker(n3xb_makers, purse_accessor, maker_file_result?.path())
            {
                makers.insert(trade_uuid, maker);
            }
        }
        Ok(makers)
    }

    fn restore_sell_maker(
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
        ) {
            Ok(maker) => maker,
            Err(error) => {
                panic!("Error restoring Sell Maker - {}", error);
                // return None;
            }
        };
        Some((trade_uuid, FatCrabMakerEnum::Sell(maker)))
    }

    fn restore_buy_takers(
        n3xb_takers: &HashMap<Uuid, TakerAccess>,
        purse_accessor: &PurseAccess,
        taker_buy_dir_path: impl AsRef<Path>,
    ) -> Result<HashMap<Uuid, FatCrabTakerEnum>, FatCrabError> {
        let mut takers = HashMap::new();
        let mut taker_files = std::fs::read_dir(taker_buy_dir_path.as_ref())?;

        while let Some(taker_file_result) = taker_files.next() {
            if let Some((trade_uuid, taker)) =
                Self::restore_buy_taker(n3xb_takers, purse_accessor, taker_file_result?.path())
            {
                takers.insert(trade_uuid, taker);
            }
        }
        Ok(takers)
    }

    fn restore_buy_taker(
        n3xb_takers: &HashMap<Uuid, TakerAccess>,
        purse_accessor: &PurseAccess,
        taker_buy_data_path: impl AsRef<Path>,
    ) -> Option<(Uuid, FatCrabTakerEnum)> {
        let file_stem = match taker_buy_data_path.as_ref().file_stem() {
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

        let n3xb_taker = match n3xb_takers.get(&trade_uuid) {
            Some(taker) => taker,
            None => return None,
        };

        let taker = match FatCrabTaker::<TakerBuy>::restore(
            n3xb_taker.clone(),
            purse_accessor.clone(),
            taker_buy_data_path,
        ) {
            Ok(taker) => taker,
            Err(error) => {
                panic!("Error restoring Buy Taker - {}", error);
                // return None;
            }
        };
        Some((trade_uuid, FatCrabTakerEnum::Buy(taker)))
    }

    fn restore_sell_takers(
        n3xb_takers: &HashMap<Uuid, TakerAccess>,
        purse_accessor: &PurseAccess,
        taker_sell_dir_path: impl AsRef<Path>,
    ) -> Result<HashMap<Uuid, FatCrabTakerEnum>, FatCrabError> {
        let mut takers = HashMap::new();
        let mut taker_files = std::fs::read_dir(taker_sell_dir_path.as_ref())?;

        while let Some(taker_file_result) = taker_files.next() {
            if let Some((trade_uuid, taker)) =
                Self::restore_sell_taker(n3xb_takers, purse_accessor, taker_file_result?.path())
            {
                takers.insert(trade_uuid, taker);
            }
        }
        Ok(takers)
    }

    fn restore_sell_taker(
        n3xb_takers: &HashMap<Uuid, TakerAccess>,
        purse_accessor: &PurseAccess,
        taker_sell_data_path: impl AsRef<Path>,
    ) -> Option<(Uuid, FatCrabTakerEnum)> {
        let file_stem = match taker_sell_data_path.as_ref().file_stem() {
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

        let n3xb_taker = match n3xb_takers.get(&trade_uuid) {
            Some(taker) => taker,
            None => return None,
        };

        let taker = match FatCrabTaker::<TakerSell>::restore(
            n3xb_taker.clone(),
            purse_accessor.clone(),
            taker_sell_data_path,
        ) {
            Ok(taker) => taker,
            Err(error) => {
                panic!("Error restoring Buy Taker - {}", error);
                // return None;
            }
        };
        Some((trade_uuid, FatCrabTakerEnum::Sell(taker)))
    }

    pub async fn reconnect(&self) -> Result<(), FatCrabError> {
        self.n3xb_manager.connect_all_relays().await?;
        Ok(())
    }

    pub fn get_network(&self) -> bitcoin::Network {
        self.network
    }

    pub async fn wallet_bip39_mnemonic(&self) -> Result<Mnemonic, FatCrabError> {
        self.purse_accessor.get_mnemonic().await
    }

    pub async fn wallet_balances(&self) -> Result<Balances, FatCrabError> {
        self.purse_accessor.get_balances().await
    }

    pub async fn wallet_generate_receive_address(&self) -> Result<Address, FatCrabError> {
        self.purse_accessor.get_rx_address().await
    }

    pub async fn wallet_send_to_address(
        &self,
        address: Address,
        sats: u64,
    ) -> Result<Txid, FatCrabError> {
        // Only allow confirmed amounts to be sent
        let balance = self.purse_accessor.get_balances().await?;

        if balance.confirmed < sats {
            return Err(FatCrabError::Simple {
                description: "Insufficient Confirmed Balance".to_string(),
            });
        }

        let funds_id = self.purse_accessor.allocate_funds(sats).await?;
        self.purse_accessor.send_funds(funds_id, address).await
    }

    pub async fn wallet_blockchain_height(&self) -> Result<u32, FatCrabError> {
        self.purse_accessor.get_height().await
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

    pub async fn get_relays(&self) -> Vec<RelayInfo> {
        self.n3xb_manager.get_relays().await
    }

    pub async fn remove_relay(&self, url: Url) -> Result<(), FatCrabError> {
        self.n3xb_manager.remove_relay(url).await?;
        Ok(())
    }

    pub async fn new_buy_maker(
        &self,
        order: &FatCrabOrder,
        fatcrab_rx_addr: impl Into<String>,
    ) -> Result<FatCrabMakerAccess<MakerBuy>, FatCrabError> {
        assert_eq!(order.order_type, FatCrabOrderType::Buy);

        let n3xb_maker = self.n3xb_manager.new_maker(order.clone().into()).await;
        let trade_uuid = order.trade_uuid.clone();

        let maker_result = FatCrabMaker::<MakerBuy>::new(
            order,
            fatcrab_rx_addr,
            n3xb_maker,
            self.purse.new_accessor(),
            self.trader_dir_path.join(MAKER_BUY_DIR_STR),
        )
        .await;

        let maker = match maker_result {
            Ok(maker) => maker,
            Err(error) => return Err(error),
        };
        let maker_accessor = maker.new_accessor();
        let maker_return_accessor = maker.new_accessor();

        let mut makers = self.makers.write().await;
        makers.insert(trade_uuid.clone(), FatCrabMakerEnum::Buy(maker));

        let mut maker_accessors = self.maker_accessors.write().await;
        maker_accessors.insert(trade_uuid, FatCrabMakerAccessEnum::Buy(maker_accessor));

        Ok(maker_return_accessor)
    }

    pub async fn new_sell_maker(
        &self,
        order: &FatCrabOrder,
    ) -> Result<FatCrabMakerAccess<MakerSell>, FatCrabError> {
        assert_eq!(order.order_type, FatCrabOrderType::Sell);

        let n3xb_maker = self.n3xb_manager.new_maker(order.clone().into()).await;
        let trade_uuid = order.trade_uuid.clone();

        let maker_result = FatCrabMaker::<MakerSell>::new(
            order,
            n3xb_maker,
            self.purse.new_accessor(),
            self.trader_dir_path.join(MAKER_SELL_DIR_STR),
        )
        .await;
        let maker = match maker_result {
            Ok(maker) => maker,
            Err(error) => return Err(error),
        };
        let maker_accessor = maker.new_accessor();
        let maker_return_accessor = maker.new_accessor();

        let mut makers = self.makers.write().await;
        makers.insert(trade_uuid.clone(), FatCrabMakerEnum::Sell(maker));

        let mut maker_accessors = self.maker_accessors.write().await;
        maker_accessors.insert(trade_uuid, FatCrabMakerAccessEnum::Sell(maker_accessor));

        Ok(maker_return_accessor)
    }

    pub async fn query_orders(
        &self,
        order_type: Option<FatCrabOrderType>,
    ) -> Result<Vec<FatCrabOrderEnvelope>, FatCrabError> {
        let custom_fatcrab_obligation_kind: ObligationKind =
            ObligationKind::Custom(FATCRAB_OBLIGATION_CUSTOM_KIND_STRING.to_string());
        let bitcoin_onchain_obligation_kind: ObligationKind =
            ObligationKind::Bitcoin(self.n3xb_network(), Some(BitcoinSettlementMethod::Onchain));

        let mut filter_tags = Vec::new();
        if let Some(order_type) = order_type {
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
        }

        let n3xb_orders = self.n3xb_manager.query_orders(filter_tags).await?;
        let orders: Vec<FatCrabOrderEnvelope> = n3xb_orders
            .into_iter()
            .map(|envelope| FatCrabOrderEnvelope {
                order: FatCrabOrder::from_n3xb_order(envelope.order.clone(), self.network).unwrap(),
                pubkey: envelope.pubkey.to_string(),
                envelope,
            })
            .collect();
        Ok(orders)
    }

    pub async fn new_buy_taker(
        &self,
        order_envelope: &FatCrabOrderEnvelope,
    ) -> Result<FatCrabTakerAccess<TakerBuy>, FatCrabError> {
        assert_eq!(order_envelope.order.order_type, FatCrabOrderType::Buy);

        let n3xb_taker = self
            .n3xb_manager
            .new_taker(
                order_envelope.envelope.clone(),
                FatCrabOffer::create_n3xb_offer(order_envelope.order.clone()),
            )
            .await
            .unwrap();
        let trade_uuid = order_envelope.order.trade_uuid.clone();

        let taker_result = FatCrabTaker::<TakerBuy>::new(
            order_envelope,
            n3xb_taker,
            self.purse.new_accessor(),
            self.trader_dir_path.join(TAKER_BUY_DIR_STR),
        )
        .await;

        let taker = match taker_result {
            Ok(taker) => taker,
            Err(err) => return Err(err),
        };

        let taker_accessor = taker.new_accessor();
        let taker_return_accessor = taker.new_accessor();

        let mut takers = self.takers.write().await;
        takers.insert(trade_uuid.clone(), FatCrabTakerEnum::Buy(taker));

        let mut taker_accessors = self.taker_accessors.write().await;
        taker_accessors.insert(trade_uuid, FatCrabTakerAccessEnum::Buy(taker_accessor));

        Ok(taker_return_accessor)
    }

    pub async fn new_sell_taker(
        &self,
        order_envelope: &FatCrabOrderEnvelope,
        fatcrab_rx_addr: impl Into<String>,
    ) -> Result<FatCrabTakerAccess<TakerSell>, FatCrabError> {
        assert_eq!(order_envelope.order.order_type, FatCrabOrderType::Sell);

        let n3xb_taker = self
            .n3xb_manager
            .new_taker(
                order_envelope.envelope.clone(),
                FatCrabOffer::create_n3xb_offer(order_envelope.order.clone()),
            )
            .await
            .unwrap();
        let trade_uuid = order_envelope.order.trade_uuid.clone();

        let taker_result = FatCrabTaker::<TakerSell>::new(
            order_envelope,
            fatcrab_rx_addr,
            n3xb_taker,
            self.purse.new_accessor(),
            self.trader_dir_path.join(TAKER_SELL_DIR_STR),
        )
        .await;

        let taker = match taker_result {
            Ok(taker) => taker,
            Err(err) => return Err(err),
        };

        let taker_accessor = taker.new_accessor();
        let taker_return_accessor = taker.new_accessor();

        let mut takers = self.takers.write().await;
        takers.insert(trade_uuid.clone(), FatCrabTakerEnum::Sell(taker));

        let mut taker_accessors = self.taker_accessors.write().await;
        taker_accessors.insert(trade_uuid, FatCrabTakerAccessEnum::Sell(taker_accessor));

        Ok(taker_return_accessor)
    }

    pub async fn get_makers(&self) -> HashMap<Uuid, FatCrabMakerAccessEnum> {
        self.maker_accessors.read().await.clone()
    }

    pub async fn get_buy_makers(&self) -> HashMap<Uuid, FatCrabMakerAccess<MakerBuy>> {
        let maker_accessors = self.maker_accessors.read().await.clone();
        let mut buy_makers = HashMap::new();
        for (uuid, maker_accessor) in maker_accessors.iter() {
            if let FatCrabMakerAccessEnum::Buy(maker_accessor) = maker_accessor {
                buy_makers.insert(uuid.clone(), maker_accessor.clone());
            }
        }
        buy_makers
    }

    pub async fn get_sell_makers(&self) -> HashMap<Uuid, FatCrabMakerAccess<MakerSell>> {
        let maker_accessors = self.maker_accessors.read().await;
        let mut sell_makers = HashMap::new();
        for (uuid, maker_accessor) in maker_accessors.iter() {
            if let FatCrabMakerAccessEnum::Sell(maker_accessor) = maker_accessor {
                sell_makers.insert(uuid.clone(), maker_accessor.clone());
            }
        }
        sell_makers
    }

    pub async fn get_buy_takers(&self) -> HashMap<Uuid, FatCrabTakerAccess<TakerBuy>> {
        let taker_accessors = self.taker_accessors.read().await;
        let mut buy_takers = HashMap::new();
        for (uuid, taker_accessor) in taker_accessors.iter() {
            if let FatCrabTakerAccessEnum::Buy(taker_accessor) = taker_accessor {
                buy_takers.insert(uuid.clone(), taker_accessor.clone());
            }
        }
        buy_takers
    }

    pub async fn get_sell_takers(&self) -> HashMap<Uuid, FatCrabTakerAccess<TakerSell>> {
        let taker_accessors = self.taker_accessors.read().await;
        let mut sell_takers = HashMap::new();
        for (uuid, taker_accessor) in taker_accessors.iter() {
            if let FatCrabTakerAccessEnum::Sell(taker_accessor) = taker_accessor {
                sell_takers.insert(uuid.clone(), taker_accessor.clone());
            }
        }
        sell_takers
    }

    pub async fn get_takers(&self) -> HashMap<Uuid, FatCrabTakerAccessEnum> {
        self.taker_accessors.read().await.clone()
    }

    pub async fn shutdown(self) -> Result<(), JoinError> {
        if let Some(error) = self.purse_accessor.shutdown().await.err() {
            warn!("Trader error shutting down Purse: {}", error);
        }
        self.purse.task_handle.await?;

        let maker_accessors = self.maker_accessors.read().await;
        for (_uuid, maker_accessor) in maker_accessors.iter() {
            if let Some(error) = maker_accessor.shutdown().await.err() {
                warn!("Trader error shutting down Maker: {}", error);
            }
        }

        let taker_accessors = self.taker_accessors.read().await;
        for (_uuid, taker_accessor) in taker_accessors.iter() {
            if let Some(error) = taker_accessor.shutdown().await.err() {
                warn!("Trader error shutting down Taker: {}", error);
            }
        }

        let mut makers = self.makers.write().await;
        for (_uuid, maker) in makers.drain() {
            maker.await_task_handle().await?;
        }
        let mut takers = self.takers.write().await;
        for (_uuid, taker) in takers.drain() {
            taker.await_task_handle().await?;
        }

        self.n3xb_manager.shutdown().await?;
        Ok(())
    }
}
