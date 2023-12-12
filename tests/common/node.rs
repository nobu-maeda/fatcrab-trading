use bitcoin::{Address, Amount, Network, Txid};
use core_rpc::{Auth, RpcApi};
use electrsd::bitcoind::BitcoinD;

pub struct Node {
    bitcoind: BitcoinD,
    bitcoind_auth: Auth,
    core_address: Address,
}

impl Node {
    pub fn new() -> Self {
        // -- Setting up background bitcoind process
        println!(">> Setting up bitcoind");

        // Start the bitcoind process - Conf::default is Regtest
        let bitcoind_conf = electrsd::bitcoind::Conf::default();

        // electrsd will automatically download the bitcoin core binaries
        let bitcoind_exe = electrsd::bitcoind::downloaded_exe_path()
            .expect("We should always have downloaded path");

        // Launch bitcoind and gather authentication access
        let bitcoind = BitcoinD::with_conf(bitcoind_exe, &bitcoind_conf).unwrap();
        let bitcoind_auth = Auth::CookieFile(bitcoind.params.cookie_file.clone());

        // Get a new core address
        let core_address = bitcoind
            .client
            .get_new_address(None, None)
            .unwrap()
            .assume_checked();

        // Generate 101 blocks and use the above address as coinbase
        bitcoind
            .client
            .generate_to_address(101, &core_address)
            .unwrap();

        println!(">> bitcoind setup complete");
        println!(
            "Available coins in Core wallet : {}",
            bitcoind.client.get_balance(None, None).unwrap()
        );

        Self {
            bitcoind,
            bitcoind_auth,
            core_address,
        }
    }

    pub fn auth(&self) -> Auth {
        self.bitcoind_auth.clone()
    }

    pub fn url(&self) -> String {
        self.bitcoind.rpc_url()
    }

    pub fn network(&self) -> Network {
        Network::Regtest
    }

    pub fn generate_blocks(&self, block_num: u64) {
        self.bitcoind
            .client
            .generate_to_address(block_num, &self.core_address)
            .unwrap();
    }

    pub fn get_spendable_balance(&self) -> u64 {
        self.bitcoind
            .client
            .get_balance(None, None)
            .unwrap()
            .to_sat()
    }

    pub fn get_receive_address(&self) -> Address {
        self.core_address.clone()
    }

    pub fn send_to_address(&self, address: Address, sats: u64) -> Txid {
        let amount = Amount::from_sat(sats);
        self.bitcoind
            .client
            .send_to_address(&address, amount, None, None, None, None, None, None)
            .unwrap()
    }
}
