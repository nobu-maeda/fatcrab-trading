use std::error::Error;
use std::fmt::{Display, Formatter, Result};

#[derive(Debug)]
pub enum FatCrabError {
    Simple {
        description: String,
    },
    N3xb {
        error: crusty_n3xb::common::error::N3xbError,
    },
    BdkBip39 {
        error: bdk::keys::bip39::Error,
    },
    Bdk {
        error: bdk::Error,
    },
}

impl Error for FatCrabError {}

impl Display for FatCrabError {
    fn fmt(&self, f: &mut Formatter) -> Result {
        let error_string: String = match self {
            FatCrabError::Simple { description } => {
                format!("FatCrab-Error | Simple - {}", description)
            }
            FatCrabError::N3xb { error } => format!("FatCrab-Error | n3xb - {}", error),
            FatCrabError::BdkBip39 { error } => format!("FatCrab-Error | bip39 - {}", error),
            FatCrabError::Bdk { error } => format!("FatCrab-Error | bdk - {}", error),
        };
        write!(f, "{}", error_string)
    }
}

impl From<crusty_n3xb::common::error::N3xbError> for FatCrabError {
    fn from(e: crusty_n3xb::common::error::N3xbError) -> FatCrabError {
        FatCrabError::N3xb { error: e }
    }
}

impl From<bdk::keys::bip39::Error> for FatCrabError {
    fn from(e: bdk::keys::bip39::Error) -> FatCrabError {
        FatCrabError::BdkBip39 { error: e }
    }
}

impl From<bdk::Error> for FatCrabError {
    fn from(e: bdk::Error) -> FatCrabError {
        FatCrabError::Bdk { error: e }
    }
}
