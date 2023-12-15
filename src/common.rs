use bitcoin::Network;
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
    }
}