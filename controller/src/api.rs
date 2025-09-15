use garde::Validate;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, Validate, JsonSchema)]
#[kube(group = "sms.dev", version = "v1", kind = "Cluster", namespaced)]
pub struct ClusterSpec {}

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, Validate, JsonSchema)]
#[kube(group = "sms.dev", version = "v1", kind = "Computer", namespaced)]
#[kube(status = "ComputerStatus")]
pub struct ComputerSpec {
    #[garde(skip)]
    pub id: String,
    #[garde(skip)]
    pub kind: ComputerKind,
    #[garde(skip)]
    pub state: ComputerInternalState,
}

#[derive(Deserialize, Serialize, Clone, Debug, Validate, Default, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComputerKind {
    #[default]
    Worker,
    Gateway,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct ComputerStatus {
    #[serde(skip)]
    pub state: ComputerInternalState,
    pub online: bool,
    pub last_heartbeat_unix_sec: Option<i64>,
}

#[derive(
    Deserialize, Serialize, Clone, Debug, PartialEq, Eq, Hash, Validate, Default, JsonSchema,
)]
pub struct ComputerInternalState {
    #[garde(skip)]
    pub label: Option<String>,
    #[garde(skip)]
    pub script: Option<String>,
}
