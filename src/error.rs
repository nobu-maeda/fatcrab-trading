use std::error::Error;
use std::fmt::{Display, Formatter, Result};

#[derive(Debug)]
pub enum FatCrabError {
    TxNotFound,
    TxUnconfirmed,
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
    Io {
        error: tokio::io::Error,
    },
    JoinError {
        error: tokio::task::JoinError,
    },
    SerdesJson {
        error: serde_json::Error,
    },
    MpscSend {
        description: String,
    },
    OneshotRecv {
        error: tokio::sync::oneshot::error::RecvError,
    },
}

impl Error for FatCrabError {}

impl Display for FatCrabError {
    fn fmt(&self, f: &mut Formatter) -> Result {
        let error_string: String = match self {
            FatCrabError::TxNotFound => "FatCrab-Error | TxNotFound".to_string(),
            FatCrabError::TxUnconfirmed => "FatCrab-Error | TxUnconfirmed".to_string(),
            FatCrabError::Simple { description } => {
                format!("FatCrab-Error | Simple - {}", description)
            }
            FatCrabError::N3xb { error } => format!("FatCrab-Error | n3xb - {}", error),
            FatCrabError::BdkBip39 { error } => format!("FatCrab-Error | bip39 - {}", error),
            FatCrabError::Bdk { error } => format!("FatCrab-Error | bdk - {}", error),
            FatCrabError::Io { error } => format!("FatCrab-Error | io - {}", error),
            FatCrabError::JoinError { error } => format!("FatCrab-Error | join - {}", error),
            FatCrabError::SerdesJson { error } => format!("FatCrab-Error | json - {}", error),
            FatCrabError::MpscSend { description } => {
                format!("FatCrab-Error | mpsc-send - {}", description)
            }
            FatCrabError::OneshotRecv { error } => {
                format!("FatCrab-Error | oneshot-recv - {}", error)
            }
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

impl From<tokio::io::Error> for FatCrabError {
    fn from(e: tokio::io::Error) -> FatCrabError {
        FatCrabError::Io { error: e }
    }
}

impl From<tokio::task::JoinError> for FatCrabError {
    fn from(e: tokio::task::JoinError) -> FatCrabError {
        FatCrabError::JoinError { error: e }
    }
}

impl From<serde_json::Error> for FatCrabError {
    fn from(e: serde_json::Error) -> FatCrabError {
        FatCrabError::SerdesJson { error: e }
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for FatCrabError {
    fn from(e: tokio::sync::mpsc::error::SendError<T>) -> FatCrabError {
        FatCrabError::MpscSend {
            description: e.to_string(),
        }
    }
}

impl From<tokio::sync::oneshot::error::RecvError> for FatCrabError {
    fn from(e: tokio::sync::oneshot::error::RecvError) -> FatCrabError {
        FatCrabError::OneshotRecv { error: e }
    }
}
