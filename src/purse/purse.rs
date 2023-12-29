use log::{info, warn};
use std::path::Path;

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
use secp256k1::{KeyPair, Secp256k1, SecretKey, XOnlyPublicKey};
use tokio::{
    select,
    sync::{mpsc, oneshot},
};
use uuid::Uuid;

use crate::common::BlockchainInfo;
use crate::error::FatCrabError;

use super::data::PurseData;

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

    pub(crate) async fn get_allocated_amount(&self) -> Result<u64, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel();
        self.tx
            .send(PurseRequest::GetAllocatedAmount { rsp_tx })
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
    pub(crate) fn new(key: SecretKey, info: BlockchainInfo, dir_path: impl AsRef<Path>) -> Self {
        let (tx, rx) = mpsc::channel::<PurseRequest>(5);
        let network = match info {
            BlockchainInfo::Electrum { network, .. } => network,
            BlockchainInfo::Rpc { network, .. } => network,
        };
        let dir_path_buf = dir_path.as_ref().to_path_buf();

        let task_handle = tokio::task::spawn(async move {
            match info {
                BlockchainInfo::Electrum { url, network } => {
                    let actor = PurseActor::<ElectrumBlockchain>::new_with_electrum(
                        rx,
                        key,
                        network,
                        url,
                        dir_path_buf.clone(),
                    );
                    actor.run().await;
                }
                BlockchainInfo::Rpc { url, auth, network } => {
                    let actor = PurseActor::<RpcBlockchain>::new_with_rpc(
                        rx,
                        key,
                        url,
                        network,
                        auth,
                        dir_path_buf.clone(),
                    );
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
    GetAllocatedAmount {
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
    data: PurseData,
}

impl PurseActor<ElectrumBlockchain> {
    pub(crate) fn new_with_electrum(
        rx: mpsc::Receiver<PurseRequest>,
        key: SecretKey,
        network: Network,
        url: String,
        dir_path: impl AsRef<Path>,
    ) -> Self {
        let client = Client::new(&url).unwrap();
        let blockchain = ElectrumBlockchain::from(client);

        Self::new(rx, key, network, blockchain, dir_path)
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
        network: Network,
        auth: Auth,
        dir_path: impl AsRef<Path>,
    ) -> Self {
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

        Self::new(rx, key, network, blockchain, dir_path)
    }
}

impl<ChainType> PurseActor<ChainType>
where
    ChainType: Blockchain + GetHeight + WalletSync,
{
    pub(crate) fn new(
        rx: mpsc::Receiver<PurseRequest>,
        key: SecretKey,
        network: Network,
        blockchain: ChainType,
        dir_path: impl AsRef<Path>,
    ) -> Self {
        let secret_bytes = key.secret_bytes();
        let xprv = ExtendedPrivKey::new_master(network, &secret_bytes).unwrap();

        let secp = Secp256k1::new();
        let keypair = KeyPair::from_secret_key(&secp, &key);
        let (pubkey, _) = XOnlyPublicKey::from_keypair(&keypair);

        let wallet = Wallet::new(
            Bip84(xprv, KeychainKind::External),
            Some(Bip84(xprv, KeychainKind::Internal)),
            network,
            MemoryDatabase::default(),
        )
        .unwrap();

        let height = blockchain.get_height().unwrap_or(0);

        let data = match PurseData::restore(pubkey.to_string(), &dir_path) {
            Ok(data) => {
                // Check if the restored data matches for height & network
                if data.network() != network {
                    panic!("Network mismatch between restored data and provided network");
                }
                if data.height() > height {
                    warn!("Block height is higher in restored data than provided network");
                }
                data
            }
            Err(err) => {
                info!(
                    "PurseData not restored from disk. Creating new PurseData - {}",
                    err
                );
                PurseData::new(pubkey.to_string(), network, height, &dir_path)
            }
        };

        Self {
            rx,
            secret_key: key,
            wallet,
            blockchain,
            data,
        }
    }

    async fn run(mut self) {
        loop {
            select! {
                    Some(request) = self.rx.recv() => {
                        match request {
                            PurseRequest::GetMnemonic { rsp_tx } => {
                                self.get_mnemonic(rsp_tx);
                            }
                            PurseRequest::GetRxAddress { rsp_tx } => {
                                self.get_rx_address(rsp_tx);
                            }
                            PurseRequest::GetSpendableBalance { rsp_tx } => {
                                self.get_spendable_balance(rsp_tx);
                            }
                            PurseRequest::GetAllocatedAmount { rsp_tx } => {
                                self.get_allocated_amount(rsp_tx);
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
                                self.send_funds(&funds_id, address, rsp_tx);
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
                    },
                    else => break,

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

    fn actual_spendable_balance(&self) -> u64 {
        self.wallet.get_balance().unwrap().confirmed - self.data.total_funds_allocated()
    }

    pub(crate) fn get_spendable_balance(&self, rsp_tx: oneshot::Sender<Result<u64, FatCrabError>>) {
        match self.wallet.get_balance() {
            Ok(_balance) => rsp_tx.send(Ok(self.actual_spendable_balance())),
            Err(e) => rsp_tx.send(Err(e.into())),
        }
        .unwrap();
    }

    pub(crate) fn get_allocated_amount(&self, rsp_tx: oneshot::Sender<Result<u64, FatCrabError>>) {
        rsp_tx.send(Ok(self.data.total_funds_allocated())).unwrap();
    }

    pub(crate) fn allocate_funds(
        &self,
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
        self.data
            .allocated_funds(&funds_id, sats, self.blockchain.get_height().ok());
        rsp_tx.send(Ok(funds_id)).unwrap();
    }

    pub(crate) fn free_funds(
        &self,
        funds_id: Uuid,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    ) {
        if self
            .data
            .deallocate_funds(&funds_id, self.blockchain.get_height().ok())
            .is_some()
        {
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
        &self,
        funds_id: &Uuid,
        address: Address,
        rsp_tx: oneshot::Sender<Result<Txid, FatCrabError>>,
    ) {
        let sats = match self.data.get_allocated_funds(funds_id) {
            Some(sats) => sats,
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

        if self
            .data
            .deallocate_funds(funds_id, self.blockchain.get_height().ok())
            .is_some()
        {
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

    pub(crate) fn shutdown(self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        self.data.terminate();
        rsp_tx.send(Ok(())).unwrap();
    }
}
