use std::{any::Any, str::FromStr};

use bitcoin::{Address, Network};
use core_rpc::Auth;

pub(crate) static FATCRAB_OBLIGATION_CUSTOM_KIND_STRING: &str = "FatCrab";

#[derive(Debug, Clone)]
pub enum BlockchainInfo {
    Electrum {
        url: String,
        network: Network,
    },
    Rpc {
        url: String,
        auth: Auth,
        network: Network,
    },
}

#[derive(Debug, Clone)]
pub struct Balances {
    /// All coinbase outputs not yet matured
    pub immature: u64,
    /// Unconfirmed UTXOs generated by a wallet tx
    pub trusted_pending: u64,
    /// Unconfirmed UTXOs received from an external wallet
    pub untrusted_pending: u64,
    /// Confirmed and immediately spendable balance
    pub confirmed: u64,
    /// Funds allocated by Trader
    pub allocated: u64,
}

impl Balances {
    pub fn from(b: bdk::Balance, allocated: u64) -> Self {
        Self {
            immature: b.immature,
            trusted_pending: b.trusted_pending,
            untrusted_pending: b.untrusted_pending,
            confirmed: b.confirmed,
            allocated,
        }
    }
}

pub fn parse_address(address: impl AsRef<str>, network: Network) -> Address {
    let btc_unchecked_addr = Address::from_str(address.as_ref()).unwrap();
    let btc_addr = match btc_unchecked_addr.require_network(network) {
        Ok(addr) => addr,
        Err(error) => {
            panic!(
                "Address {:?} is not {} - {}",
                address.as_ref(),
                network,
                error.to_string()
            );
        }
    };
    btc_addr
}

#[typetag::serde(tag = "type")]
pub trait SerdeGenericTrait: Send + Sync {
    fn any_ref(&self) -> &dyn Any;
}

impl dyn SerdeGenericTrait {
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.any_ref().downcast_ref()
    }
}
