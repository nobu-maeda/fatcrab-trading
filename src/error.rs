use std::error::Error;
use std::fmt::{Display, Formatter, Result};

use crusty_n3xb::common::error::N3xbError;

#[derive(Debug)]
pub enum FatCrabError {
    Simple { description: String },
    N3xb { error: N3xbError },
}

impl Error for FatCrabError {}

impl Display for FatCrabError {
    fn fmt(&self, f: &mut Formatter) -> Result {
        let error_string: String = match self {
            FatCrabError::Simple { description } => {
                format!("FatCrab-Error | Simple - {}", description)
            }
            FatCrabError::N3xb { error } => format!("FatCrab-Error | n3xb - {}", error),
        };
        write!(f, "{}", error_string)
    }
}

impl From<N3xbError> for FatCrabError {
    fn from(e: N3xbError) -> FatCrabError {
        FatCrabError::N3xb { error: e }
    }
}
