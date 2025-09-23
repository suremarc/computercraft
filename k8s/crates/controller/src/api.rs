use std::path::PathBuf;

use garde::Validate;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, Validate, JsonSchema)]
#[kube(group = "smcs.dev", version = "v1", kind = "Computer", namespaced)]
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
    group = "smcs.dev",
    version = "v1",
    kind = "ComputerCluster",
    namespaced
)]
pub struct ComputerClusterSpec {
    #[garde(skip)]
    pub gateway: Option<ComputerGatewaySpec>,
}

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, Validate, JsonSchema)]
#[kube(
    group = "smcs.dev",
    version = "v1",
    kind = "ComputerGateway",
    namespaced
)]
pub struct ComputerGatewaySpec {
    #[garde(skip)]
    pub routes: Vec<HttpOverRednetRoute>,
    #[garde(skip)]
    pub links: Vec<ComputerGatewayLink>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Validate, JsonSchema)]
pub struct ComputerGatewayLink {
    #[garde(skip)]
    host_id: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, Hash, Validate, JsonSchema)]
pub struct HttpOverRednetRoute {
    #[garde(skip)]
    pub backend: RednetBackend,
    #[garde(skip)]
    pub prefix: PathBuf,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, Hash, Validate, JsonSchema)]
#[serde(rename_all = "camelCase")]
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

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, Hash, JsonSchema)]
pub struct RednetGatewayConfigMapData {
    pub routes: Vec<HttpOverRednetRoute>,
}
