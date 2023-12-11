use std::collections::HashMap;

use bdk::{
    bitcoin::{bip32::ExtendedPrivKey, Network},
    blockchain::{
        rpc::Auth as BdkAuth, Blockchain, ConfigurableBlockchain, GetHeight, RpcBlockchain,
        RpcConfig,
    },
    database::MemoryDatabase,
    keys::bip39::Mnemonic,
    template::Bip84,
    wallet::AddressIndex,
    KeychainKind, SignOptions, SyncOptions, TransactionDetails, Wallet,
};

use bitcoin::{psbt::PartiallySignedTransaction, Address, Txid};
use core_rpc::Auth;
use secp256k1::SecretKey;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::error::FatCrabError;

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

    pub(crate) async fn allocate_tx(
        &self,
        address: Address,
        sats: u64,
    ) -> Result<Txid, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel();
        self.tx
            .send(PurseRequest::AllocateTx {
                address,
                sats,
                rsp_tx,
            })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub(crate) async fn free_tx(&self, txid: Txid) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel();
        self.tx
            .send(PurseRequest::FreeTx { txid, rsp_tx })
            .await
            .unwrap();
        rsp_rx.await.unwrap()
    }

    pub(crate) async fn send_tx(&self, txid: Txid) -> Result<(), FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel();
        self.tx
            .send(PurseRequest::SendTx { txid, rsp_tx })
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
}

pub(crate) struct Purse {
    tx: mpsc::Sender<PurseRequest>,
    task_handle: tokio::task::JoinHandle<()>,
    network: Network,
}

impl Purse {
    pub(crate) fn new(
        key: SecretKey,
        url: impl Into<String>,
        auth: Auth,
        network: Network,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<PurseRequest>(10);
        let mut actor = PurseActor::new(rx, key, url, auth, network);
        let task_handle = tokio::spawn(async move { actor.run().await });
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
    AllocateTx {
        address: Address,
        sats: u64,
        rsp_tx: oneshot::Sender<Result<Txid, FatCrabError>>,
    },
    FreeTx {
        txid: Txid,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    SendTx {
        txid: Txid,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
    GetTxConf {
        txid: Txid,
        rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>,
    },
    SyncBlockchain {
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
}

struct PurseActor {
    rx: mpsc::Receiver<PurseRequest>,
    secret_key: SecretKey,
    wallet: Wallet<MemoryDatabase>, // TOOD: This can't be Memory Database forever
    blockchain: RpcBlockchain,
    allocated_txs: HashMap<Txid, (PartiallySignedTransaction, TransactionDetails)>,
}

impl PurseActor {
    fn rpc_to_bdk_auth(auth: Auth) -> BdkAuth {
        match auth {
            Auth::UserPass(username, password) => BdkAuth::UserPass { username, password },
            Auth::CookieFile(path) => BdkAuth::Cookie { file: path },
            Auth::None => BdkAuth::None,
        }
    }

    pub(crate) fn new(
        rx: mpsc::Receiver<PurseRequest>,
        key: SecretKey,
        url: impl Into<String>,
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
            allocated_txs: HashMap::new(),
        }
    }

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
                PurseRequest::AllocateTx {
                    address,
                    sats,
                    rsp_tx,
                } => {
                    self.allocate_tx(address, sats, rsp_tx);
                }
                PurseRequest::FreeTx { txid, rsp_tx } => {
                    self.free_tx(txid, rsp_tx);
                }
                PurseRequest::SendTx { txid, rsp_tx } => {
                    self.send_tx(txid, rsp_tx);
                }
                PurseRequest::GetTxConf { txid, rsp_tx } => {
                    self.get_tx_conf(txid, rsp_tx);
                }
                PurseRequest::SyncBlockchain { rsp_tx } => {
                    self.sync_blockchain(rsp_tx);
                }
            }
        }
    }

    pub fn get_mnemonic(&self, rsp_tx: oneshot::Sender<Result<Mnemonic, FatCrabError>>) {
        match Mnemonic::from_entropy(&self.secret_key.secret_bytes()) {
            Ok(mnemonic) => rsp_tx.send(Ok(mnemonic)),
            Err(e) => rsp_tx.send(Err(e.into())),
        }
        .unwrap();
    }

    pub fn get_rx_address(&self, rsp_tx: oneshot::Sender<Result<Address, FatCrabError>>) {
        match self.wallet.get_address(AddressIndex::New) {
            Ok(address) => rsp_tx.send(Ok(address.address)),
            Err(e) => rsp_tx.send(Err(e.into())),
        }
        .unwrap();
    }

    fn total_allocated_sats(&self) -> u64 {
        self.allocated_txs
            .iter()
            .map(|(_, (_, tx_details))| {
                tx_details.sent + tx_details.fee.unwrap_or(0) - tx_details.received
            })
            .sum()
    }

    fn actual_spendable_balance(&self) -> u64 {
        self.wallet.get_balance().unwrap().confirmed - self.total_allocated_sats()
    }

    pub fn get_spendable_balance(&self, rsp_tx: oneshot::Sender<Result<u64, FatCrabError>>) {
        match self.wallet.get_balance() {
            Ok(balance) => rsp_tx.send(Ok(self.actual_spendable_balance())),
            Err(e) => rsp_tx.send(Err(e.into())),
        }
        .unwrap();
    }

    pub fn allocate_tx(
        &self,
        address: Address,
        sats: u64,
        rsp_tx: oneshot::Sender<Result<Txid, FatCrabError>>,
    ) {
        if self.actual_spendable_balance() < sats {
            rsp_tx
                .send(Err(FatCrabError::Simple {
                    description: "Insufficient Funds".to_string(),
                }))
                .unwrap();
            return;
        }

        let mut tx_builder = self.wallet.build_tx();
        tx_builder.set_recipients(vec![(address.script_pubkey(), sats)]);

        match tx_builder.finish() {
            Ok((psbt, tx_details)) => {
                let txid = tx_details.txid;
                self.allocated_txs.insert(txid, (psbt, tx_details));
                rsp_tx.send(Ok(txid)).unwrap();
            }
            Err(e) => {
                rsp_tx.send(Err(e.into())).unwrap();
                return;
            }
        };
    }

    pub fn free_tx(&self, txid: Txid, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        if self.allocated_txs.remove(&txid).is_some() {
            rsp_tx.send(Ok(())).unwrap();
        } else {
            rsp_tx
                .send(Err(FatCrabError::Simple {
                    description: "Could not find allocated Tx".to_string(),
                }))
                .unwrap();
        }
    }

    pub fn send_tx(&self, txid: Txid, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        let mut psbt = if let Some((psbt, _)) = self.allocated_txs.get(&txid) {
            psbt
        } else {
            rsp_tx
                .send(Err(FatCrabError::Simple {
                    description: "Could not find allocated Tx".to_string(),
                }))
                .unwrap();
            return;
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

        if self.allocated_txs.remove(&txid).is_some() {
            rsp_tx.send(Ok(())).unwrap();
        } else {
            rsp_tx
                .send(Err(FatCrabError::Simple {
                    description: "Could not find allocated Tx".to_string(),
                }))
                .unwrap();
        }
    }

    pub fn get_tx_conf(&self, txid: Txid, rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>) {
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
                rsp_tx.send(Err(e.into()));
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
            rsp_tx.send(Err(FatCrabError::Simple {
                description:
                    "Tx confirmation height greater than current blockchain height. Unexpected"
                        .to_string(),
            }));
        } else {
            rsp_tx.send(Ok(height - conf_height));
        }
    }

    pub fn sync_blockchain(&self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        match self.wallet.sync(&self.blockchain, SyncOptions::default()) {
            Ok(_) => rsp_tx.send(Ok(())),
            Err(e) => rsp_tx.send(Err(e.into())),
        }
        .unwrap();
    }
}
