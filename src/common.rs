use log::{error, trace};
use std::{any::Any, path::Path, str::FromStr, sync::Arc};

use bitcoin::{Address, Network};
use core_rpc::Auth;

use tokio::{
    io::AsyncWriteExt,
    select,
    sync::{mpsc, RwLock, RwLockReadGuard},
};

use crate::error::FatCrabError;

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
