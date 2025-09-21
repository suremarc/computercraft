use std::path::PathBuf;

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
    #[serde(flatten)]
    pub state: ComputerInternalState,
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

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, Validate, JsonSchema)]
#[kube(
    group = "sms.dev",
    version = "v1",
    kind = "ComputerGateway",
    namespaced
)]
pub struct ComputerGatewaySpec {
    #[garde(skip)]
    pub host_id: String,
}

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, Validate, JsonSchema)]
#[kube(
    group = "sms.dev",
    version = "v1",
    kind = "HTTPOverRednetRoute",
    namespaced
)]
pub struct HttpOverRednetRouteSpec {
    #[garde(skip)]
    backend: RednetBackend,
    #[garde(skip)]
    prefix: PathBuf,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, Hash, Validate, JsonSchema)]
#[serde(tag = "kind")]
pub enum RednetBackend {
    Anycast {
        #[garde(skip)]
        protocol: String,
    },
    Computer {
        #[garde(skip)]
        id: String,
        #[garde(skip)]
        protocol: Option<String>,
    },
    Hostname {
        #[garde(skip)]
        protocol: String,
        #[garde(skip)]
        host: String,
    },
}
