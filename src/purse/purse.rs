use std::path::Path;
use tracing::{error, trace, warn};

use bdk::{
    bitcoin::{bip32::ExtendedPrivKey, Network},
    blockchain::{
        rpc::Auth as BdkAuth, Blockchain, ConfigurableBlockchain, ElectrumBlockchain,
        RpcBlockchain, RpcConfig,
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

use crate::common::{Balances, BlockchainInfo};
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

    pub(crate) async fn get_balances(&self) -> Result<Balances, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel();
        self.tx
            .send(PurseRequest::GetBalances { rsp_tx })
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

    pub(crate) async fn get_height(&self) -> Result<u32, FatCrabError> {
        let (rsp_tx, rsp_rx) = oneshot::channel();
        self.tx
            .send(PurseRequest::GetHeight { rsp_tx })
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
    GetBalances {
        rsp_tx: oneshot::Sender<Result<Balances, FatCrabError>>,
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
    GetHeight {
        rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>,
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

struct PurseActor<ChainType: Blockchain> {
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
    ChainType: Blockchain,
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
        let data = PurseData::new(pubkey.to_string(), network, height, &dir_path);

        // Check if the restored data matches for height & network
        if data.network() != network {
            panic!("Network mismatch between restored data and provided network");
        }
        if data.height() > height {
            warn!("Block height is higher in restored data than provided network");
        }

        wallet.sync(&blockchain, SyncOptions::default()).unwrap();

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
                            PurseRequest::GetBalances { rsp_tx } => {
                                self.get_balances(rsp_tx);
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
                            PurseRequest::GetHeight { rsp_tx } => {
                                self.get_height(rsp_tx);
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

    fn allocatable_balance(&self) -> u64 {
        let balance = self.wallet.get_balance().unwrap();
        balance.confirmed + balance.trusted_pending - self.data.total_funds_allocated()
    }

    pub(crate) fn get_balances(&self, rsp_tx: oneshot::Sender<Result<Balances, FatCrabError>>) {
        match self.wallet.get_balance() {
            Ok(balance) => rsp_tx.send(Ok(Balances::from(
                balance,
                self.data.total_funds_allocated(),
            ))),
            Err(e) => rsp_tx.send(Err(e.into())),
        }
        .unwrap();
    }

    pub(crate) fn allocate_funds(
        &self,
        sats: u64,
        rsp_tx: oneshot::Sender<Result<Uuid, FatCrabError>>,
    ) {
        if self.allocatable_balance() < sats {
            rsp_tx
                .send(Err(FatCrabError::Simple {
                    description: "Insufficient Funds".to_string(),
                }))
                .unwrap();
            return;
        }

        let funds_id = Uuid::new_v4();
        self.data
            .allocate_funds(&funds_id, sats, Some(self.data.height()));
        rsp_tx.send(Ok(funds_id)).unwrap();
    }

    pub(crate) fn free_funds(
        &self,
        funds_id: Uuid,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    ) {
        if self
            .data
            .deallocate_funds(&funds_id, Some(self.data.height()))
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
        trace!("PurseActor::send_funds()");

        let sats = match self.data.get_allocated_funds(funds_id) {
            Some(sats) => sats,
            None => {
                error!(
                    "PurseActor::send_funds() - Could not find allocated funds with ID {}",
                    funds_id
                );

                rsp_tx
                    .send(Err(FatCrabError::Simple {
                        description: format!("Could not find allocated funds with ID {}", funds_id),
                    }))
                    .unwrap();
                return;
            }
        };

        trace!(
            "PureActor::send_funds() - Sending {} sats to {}",
            sats,
            address
        );

        let mut tx_builder = self.wallet.build_tx();
        tx_builder.set_recipients(vec![(address.script_pubkey(), sats)]);

        if self.wallet.network() != Network::Regtest {
            let confirm_in_blocks = 5;
            let fee_rate = match self.blockchain.estimate_fee(confirm_in_blocks) {
                Ok(fee_rate) => fee_rate,
                Err(e) => {
                    error!(
                        "PurseActor::send_funds() - Error estimating fee rate: {}",
                        e
                    );

                    rsp_tx.send(Err(e.into())).unwrap();
                    return;
                }
            };

            trace!(
                "PurseActor::send_funds() - Estimated fee rate: {:?} sats/vbyte to confirm in {} blocks",
                fee_rate, confirm_in_blocks
            );
            tx_builder.fee_rate(fee_rate).enable_rbf();
        }

        let mut psbt = match tx_builder.finish() {
            Ok((psbt, _)) => psbt,
            Err(e) => {
                error!(
                    "PurseActor::send_funds() - Error building transaction: {}",
                    e
                );
                rsp_tx.send(Err(e.into())).unwrap();
                return;
            }
        };

        let signopt: SignOptions = SignOptions {
            assume_height: None,
            ..Default::default()
        };

        if let Some(error) = self.wallet.sign(&mut psbt, signopt).err() {
            error!(
                "PurseActor::send_funds() - Error signing transaction: {}",
                error
            );
            rsp_tx.send(Err(error.into())).unwrap();
            return;
        }
        let tx = psbt.extract_tx();

        trace!(
            "PurseActor::send_funds() - Broadcasting transaction: {}",
            tx.txid()
        );

        if let Some(error) = self.blockchain.broadcast(&tx).err() {
            error!(
                "PurseActor::send_funds() - Error broadcasting transaction: {}",
                error
            );
            rsp_tx.send(Err(error.into())).unwrap();
            return;
        }

        trace!("PurseActor::send_funds() - Transaction sent: {}", tx.txid());

        if self
            .data
            .deallocate_funds(funds_id, Some(self.data.height()))
            .is_some()
        {
            trace!("PurseActor::send_funds() - Funds {} deallocated", funds_id);
            rsp_tx.send(Ok(tx.txid())).unwrap();
        } else {
            error!(
                "PurseActor::send_funds() - Could not find allocated funds with ID {}",
                funds_id
            );
            rsp_tx
                .send(Err(FatCrabError::Simple {
                    description: format!("Could not find allocated funds with ID {}", funds_id),
                }))
                .unwrap();
        }
    }

    pub(crate) fn get_height(&self, rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>) {
        match self.blockchain.get_height() {
            Ok(height) => rsp_tx.send(Ok(height)),
            Err(e) => rsp_tx.send(Err(e.into())),
        }
        .unwrap();
    }

    pub(crate) fn get_tx_conf(
        &self,
        txid: Txid,
        rsp_tx: oneshot::Sender<Result<u32, FatCrabError>>,
    ) {
        let height = self.data.height();

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
            rsp_tx.send(Ok(height - conf_height + 1)).unwrap();
        }
    }

    pub(crate) fn sync_blockchain(&self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        match self.wallet.sync(&self.blockchain, SyncOptions::default()) {
            Ok(_) => {
                let height = self.blockchain.get_height().unwrap();
                self.data.set_height(height);
                rsp_tx.send(Ok(()))
            }
            Err(e) => rsp_tx.send(Err(e.into())),
        }
        .unwrap();
    }

    pub(crate) fn shutdown(self, rsp_tx: oneshot::Sender<Result<(), FatCrabError>>) {
        self.data.terminate();
        rsp_tx.send(Ok(())).unwrap();
    }
}
