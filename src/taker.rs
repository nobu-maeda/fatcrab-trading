use std::sync::mpsc;

use tokio::sync::oneshot;

use crate::error::FatCrabError;

pub struct FatCrabTakerAccess {}

pub struct FatCrabTaker {}

enum FatCrabTakerRequest {
    RegisterNotifTx {
        tx: mpsc::Sender<String>,
        rsp_tx: oneshot::Sender<Result<(), FatCrabError>>,
    },
}
