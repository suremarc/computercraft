/// K8s API objects
pub mod api;

/// K8s reconciliation logic
pub mod reconciler;

use thiserror::Error;
use tokio::sync::watch::error::SendError;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Kube error: {0}")]
    Kube(#[from] kube::Error),
    #[error("No peers available for cluster: {0}")]
    ClusterUnavailable(#[from] SendError<Vec<GatewayCommand>>),
    #[error("Missing field in object reference")]
    MissingField,
}

pub type Result<T> = std::result::Result<T, Error>;

/// Commands that can be sent to gateways
pub enum GatewayCommand {
    #[allow(unused)]
    Wake { computer_id: String },
}
