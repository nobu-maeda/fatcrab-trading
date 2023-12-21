use std::collections::HashMap;

use bdk::{
    bitcoin::{bip32::ExtendedPrivKey, Network},
    blockchain::{
        rpc::Auth as BdkAuth, Blockchain, ConfigurableBlockchain, ElectrumBlockchain, GetHeight,
        RpcBlockchain, RpcConfig, WalletSync,
    },
    database::MemoryDatabase,
    electrum_client::Client,
    keys::bip39::Mnemonic,
    template::Bip84,
    wallet::AddressIndex,
    KeychainKind, SignOptions, SyncOptions, Wallet,
};

use bitcoin::{Address, Txid};
use core_rpc::Auth;
use secp256k1::SecretKey;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::common::BlockchainInfo;
use crate::error::FatCrabError;

#[derive(Clone, Debug)]
pub(crate) struct PurseAccess {
    tx: mpsc::Sender<PurseRequest>,
    pub(crate) network: Network,
}

impl PurseAccess {
    pub(crate) async fn get_mnemonic(&self) -> Result<Mnemonic, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel();
        self.tx
            .send(PurseRequest::GetMnemonic { rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub(crate) async fn get_rx_address(&self) -> Result<Address, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel();
        self.tx
            .send(PurseRequest::GetRxAddress { rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub(crate) async fn get_spendable_balance(&self) -> Result<u64, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel();
        self.tx
            .send(PurseRequest::GetSpendableBalance { rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub(crate) async fn allocate_funds(&self, sats: u64) -> Result<Uuid, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel();
        self.tx
            .send(PurseRequest::AllocateFunds { sats, rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub(crate) async fn free_funds(&self, funds_id: Uuid) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel();
        self.tx
            .send(PurseRequest::FreeFunds { funds_id, rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub(crate) async fn send_funds(
        &self,
        funds_id: Uuid,
        address: Address,
    ) -> Result<Txid, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel();
        self.tx
            .send(PurseRequest::SendFunds {
                funds_id,
                address,
                rsp_tx,
            })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub(crate) async fn get_tx_conf(&self, txid: Txid) -> Result<u32, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel();
        self.tx
            .send(PurseRequest::GetTxConf { txid, rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub(crate) async fn sync_blockchain(&self) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel();
        self.tx
            .send(PurseRequest::SyncBlockchain { rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub(crate) async fn shutdown(self) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel::<Result<(), FatCrabError>>();
        let request = PurseRequest::Shutdown { rsp_tx };
        self.tx.send(request).await.unwrap();
        rsp_rx.await.unwrap()
    }
}

pub(crate) struct Purse {
    tx: mpsc::Sender<PurseRequest>,
    pub(crate) task_handle: tokio::task::JoinHandle<()>,
    network: Network,
}

impl Purse {
    pub(crate) fn new(key: SecretKey, info: BlockchainInfo) -> Self {
        let (tx, rx) = mpsc::channel::<PurseRequest>(10);
        let network = match info {
            BlockchainInfo::Electrum { network, .. } => network,
            BlockchainInfo::Rpc { network, .. } => network,
        };

        let task_handle = tokio::spawn(async move {
            match info {
                BlockchainInfo::Electrum { url, network } => {
                    let mut actor =
                        PurseActor::<ElectrumBlockchain>::new_with_electrum(rx, key, url, network);
                    actor.run().await;
                }
                BlockchainInfo::Rpc { url, auth, network } => {
                    let mut actor =
                        PurseActor::<RpcBlockchain>::new_with_rpc(rx, key, url, auth, network);
                    actor.run().await;
                }
            }
        });

        Self {
            tx,
            task_handle,
            network,
        }
    }

    pub(crate) fn new_accessor(&self) -> PurseAccess {
        PurseAccess {
            tx: self.tx.clone(),
            network: self.network,
        }
    }
}
enum PurseRequest {
    GetMnemonic {
        rsp_tx: oneshot::Sender<Result<Mnemonic, FatCrabError>>,
    },
    GetRxAddress {
        rsp_tx: oneshot::Sender<Result<Address, FatCrabError>>,
    },
    GetSpendableBalance {
        rsp_tx: oneshot::Sender<Result<u64, FatCrabError>>,
    },
    AllocateFunds {
        sats: u64,
        rsp_tx: oneshot::Sender<Result<Uuid, FatCrabError>>,
    },
    FreeFunds {
        funds_id: Uuid,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    SendFunds {
        funds_id: Uuid,
        address: Address,
        rsp_tx: oneshot::Sender<Result<Txid, FatCrabError>>,
    },
    GetTxConf {
        txid: Txid,
        rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>,
    },
    SyncBlockchain {
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    Shutdown {
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
}

struct PurseActor<ChainType: Blockchain + GetHeight + WalletSync> {
    rx: mpsc::Receiver<PurseRequest>,
    secret_key: SecretKey,
    wallet: Wallet<MemoryDatabase>, // TOOD: This can't be Memory Database forever
    blockchain: ChainType,
    allocated_funds: HashMap<Uuid, u64>,
}

impl PurseActor<ElectrumBlockchain> {
    pub(crate) fn new_with_electrum(
        rx: mpsc::Receiver<PurseRequest>,
        key: SecretKey,
        url: String,
        network: Network,
    ) -> Self {
        let secret_bytes = key.secret_bytes();
        let xprv = ExtendedPrivKey::new_master(network, &secret_bytes).unwrap();

        let wallet = Wallet::new(
            Bip84(xprv, KeychainKind::External),
            Some(Bip84(xprv, KeychainKind::Internal)),
            network,
            MemoryDatabase::default(),
        )
        .unwrap();

        let client = Client::new(&url).unwrap();
        let blockchain = ElectrumBlockchain::from(client);

        Self {
            rx,
            secret_key: key,
            wallet,
            blockchain,
            allocated_funds: HashMap::new(),
        }
    }
}

impl PurseActor<RpcBlockchain> {
    fn rpc_to_bdk_auth(auth: Auth) -> BdkAuth {
        match auth {
            Auth::UserPass(username, password) => BdkAuth::UserPass { username, password },
            Auth::CookieFile(path) => BdkAuth::Cookie { file: path },
            Auth::None => BdkAuth::None,
        }
    }

    pub(crate) fn new_with_rpc(
        rx: mpsc::Receiver<PurseRequest>,
        key: SecretKey,
        url: String,
        auth: Auth,
        network: Network,
    ) -> Self {
        let secret_bytes = key.secret_bytes();
        let xprv = ExtendedPrivKey::new_master(network, &secret_bytes).unwrap();

        let wallet = Wallet::new(
            Bip84(xprv, KeychainKind::External),
            Some(Bip84(xprv, KeychainKind::Internal)),
            network,
            MemoryDatabase::default(),
        )
        .unwrap();

        // Create a RPC configuration of the running bitcoind backend we created in last step
        // Note: If you are using custom regtest node, use the appropriate url and auth
        let rpc_config = RpcConfig {
            url: url.into(),
            auth: Self::rpc_to_bdk_auth(auth),
            network,
            wallet_name: format!("Purse-{}", Uuid::new_v4().to_string()),
            sync_params: None,
        };

        // Use the above configuration to create a RPC blockchain backend
        let blockchain = RpcBlockchain::from_config(&rpc_config).unwrap();

        Self {
            rx,
            secret_key: key,
            wallet,
            blockchain,
            allocated_funds: HashMap::new(),
        }
    }
}

impl<ChainType> PurseActor<ChainType>
where
    ChainType: Blockchain + GetHeight + WalletSync,
{
    async fn run(&mut self) {
        while let Some(req) = self.rx.recv().await {
            match req {
                PurseRequest::GetMnemonic { rsp_tx } => {
                    self.get_mnemonic(rsp_tx);
                }
                PurseRequest::GetRxAddress { rsp_tx } => {
                    self.get_rx_address(rsp_tx);
                }
                PurseRequest::GetSpendableBalance { rsp_tx } => {
                    self.get_spendable_balance(rsp_tx);
                }
                PurseRequest::AllocateFunds { sats, rsp_tx } => {
                    self.allocate_funds(sats, rsp_tx);
                }
                PurseRequest::FreeFunds { funds_id, rsp_tx } => {
                    self.free_funds(funds_id, rsp_tx);
                }
                PurseRequest::SendFunds {
                    funds_id,
                    address,
                    rsp_tx,
                } => {
                    self.send_funds(funds_id, address, rsp_tx);
                }
                PurseRequest::GetTxConf { txid, rsp_tx } => {
                    self.get_tx_conf(txid, rsp_tx);
                }
                PurseRequest::SyncBlockchain { rsp_tx } => {
                    self.sync_blockchain(rsp_tx);
                }
                PurseRequest::Shutdown { rsp_tx } => {
                    self.shutdown(rsp_tx);
                    return;
                }
            }
        }
    }

    pub(crate) fn get_mnemonic(&self, rsp_tx: oneshot::Sender<Result<Mnemonic, FatCrabError>>) {
        match Mnemonic::from_entropy(&self.secret_key.secret_bytes()) {
            Ok(mnemonic) => rsp_tx.send(Ok(mnemonic)),
            Err(e) => rsp_tx.send(Err(e.into())),
        }
        .unwrap();
    }

    pub(crate) fn get_rx_address(&self, rsp_tx: oneshot::Sender<Result<Address, FatCrabError>>) {
        match self.wallet.get_address(AddressIndex::New) {
            Ok(address) => rsp_tx.send(Ok(address.address)),
            Err(e) => rsp_tx.send(Err(e.into())),
        }
        .unwrap();
    }

    fn total_allocated_sats(&self) -> u64 {
        self.allocated_funds.values().sum()
    }

    fn actual_spendable_balance(&self) -> u64 {
        self.wallet.get_balance().unwrap().confirmed - self.total_allocated_sats()
    }

    pub(crate) fn get_spendable_balance(&self, rsp_tx: oneshot::Sender<Result<u64, FatCrabError>>) {
        match self.wallet.get_balance() {
            Ok(_balance) => rsp_tx.send(Ok(self.actual_spendable_balance())),
            Err(e) => rsp_tx.send(Err(e.into())),
        }
        .unwrap();
    }

    pub(crate) fn allocate_funds(
        &mut self,
        sats: u64,
        rsp_tx: oneshot::Sender<Result<Uuid, FatCrabError>>,
    ) {
        if self.actual_spendable_balance() < sats {
            rsp_tx
                .send(Err(FatCrabError::Simple {
                    description: "Insufficient Funds".to_string(),
                }))
                .unwrap();
            return;
        }

        let funds_id = Uuid::new_v4();
        self.allocated_funds.insert(funds_id.clone(), sats);
        rsp_tx.send(Ok(funds_id)).unwrap();
    }

    pub(crate) fn free_funds(
        &mut self,
        funds_id: Uuid,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    ) {
        if self.allocated_funds.remove(&funds_id).is_some() {
            rsp_tx.send(Ok(())).unwrap();
        } else {
            rsp_tx
                .send(Err(FatCrabError::Simple {
                    description: format!("Could not find allocated funds with ID {}", funds_id),
                }))
                .unwrap();
        }
    }

    pub(crate) fn send_funds(
        &mut self,
        funds_id: Uuid,
        address: Address,
        rsp_tx: oneshot::Sender<Result<Txid, FatCrabError>>,
    ) {
        let sats = match self.allocated_funds.get(&funds_id) {
            Some(sats) => *sats,
            None => {
                rsp_tx
                    .send(Err(FatCrabError::Simple {
                        description: format!("Could not find allocated funds with ID {}", funds_id),
                    }))
                    .unwrap();
                return;
            }
        };

        let mut tx_builder = self.wallet.build_tx();
        tx_builder.set_recipients(vec![(address.script_pubkey(), sats)]);

        let mut psbt = match tx_builder.finish() {
            Ok((psbt, _)) => psbt,
            Err(e) => {
                rsp_tx.send(Err(e.into())).unwrap();
                return;
            }
        };

        let signopt: SignOptions = SignOptions {
            assume_height: None,
            ..Default::default()
        };

        if let Some(error) = self.wallet.sign(&mut psbt, signopt).err() {
            rsp_tx.send(Err(error.into())).unwrap();
            return;
        }
        let tx = psbt.extract_tx();

        if let Some(error) = self.blockchain.broadcast(&tx).err() {
            rsp_tx.send(Err(error.into())).unwrap();
            return;
        }

        if self.allocated_funds.remove(&funds_id).is_some() {
            rsp_tx.send(Ok(tx.txid())).unwrap();
        } else {
            rsp_tx
                .send(Err(FatCrabError::Simple {
                    description: format!("Could not find allocated funds with ID {}", funds_id),
                }))
                .unwrap();
        }
    }

    pub(crate) fn get_tx_conf(
        &self,
        txid: Txid,
        rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>,
    ) {
        let height = match self.blockchain.get_height() {
            Ok(height) => height,
            Err(e) => {
                rsp_tx.send(Err(e.into())).unwrap();
                return;
            }
        };

        let tx_details = match self.wallet.get_tx(&txid, false) {
            Ok(tx_details) => match tx_details {
                Some(tx_details) => tx_details,
                None => {
                    rsp_tx.send(Err(FatCrabError::TxNotFound)).unwrap();
                    return;
                }
            },
            Err(e) => {
                rsp_tx.send(Err(e.into())).unwrap();
                return;
            }
        };

        let conf_height = match tx_details.confirmation_time {
            Some(block_time) => block_time.height,
            None => {
                rsp_tx.send(Err(FatCrabError::TxUnconfirmed)).unwrap();
                return;
            }
        };

        if height < conf_height {
            rsp_tx
                .send(Err(FatCrabError::Simple {
                    description:
                        "Tx confirmation height greater than current blockchain height. Unexpected"
                            .to_string(),
                }))
                .unwrap();
        } else {
            rsp_tx.send(Ok(height - conf_height)).unwrap();
        }
    }

    pub(crate) fn sync_blockchain(&self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        match self.wallet.sync(&self.blockchain, SyncOptions::default()) {
            Ok(_) => rsp_tx.send(Ok(())),
            Err(e) => rsp_tx.send(Err(e.into())),
        }
        .unwrap();
    }

    pub(crate) fn shutdown(&self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        rsp_tx.send(Ok(())).unwrap();
    }
}
