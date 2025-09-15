use std::{sync::Arc, time::Duration};

use kube::{
    Api, Client,
    api::{ListParams, Patch, PatchParams},
    runtime::{controller::Action},
};
use serde_json::json;
use tracing::{Level, instrument};

use crate::{
    Error, Result,
    api::{Cluster, Computer},
    c2::{C2Server, Command},
};

const MANAGER_NAME: &str = "computercraft-controller";

pub struct ReconcilerCtx {
    pub client: Client,
    pub c2_server: Arc<C2Server>,
}

#[instrument(level = Level::DEBUG, skip(cluster, context), fields(cluster = cluster.metadata.name.as_ref().unwrap()))]
pub async fn reconcile(cluster: Arc<Cluster>, context: Arc<ReconcilerCtx>) -> Result<Action> {
    let cluster_namespace = cluster.metadata.namespace.as_deref().unwrap();

    let cluster_name = cluster.metadata.name.as_deref().unwrap();
    let control_channel = context.c2_server.sender(cluster_namespace, cluster_name);

    let computers = Api::<Computer>::namespaced(context.client.clone(), cluster_namespace);

    let commands = compute_cluster_diff_and_set_statuses(&computers, cluster.as_ref()).await?;
    if commands.is_empty() {
        // The cluster is in a good state, check again in 5 minutes
        return Ok(Action::requeue(Duration::from_secs(300)));
    }

    control_channel.send(commands)?;

    // Check again in 10 seconds
    Ok(Action::requeue(Duration::from_secs(10)))
}

async fn compute_cluster_diff_and_set_statuses(computers: &Api<Computer>, cluster: &Cluster) -> Result<Vec<Command>> {
    let cluster_name = cluster.metadata.name.as_deref().unwrap();

    // List all computers belonging to this cluster
    let computers_for_cluster = computers.list(&ListParams::default()).await?;

    if computers_for_cluster.items.is_empty() {
        tracing::info!("No computers found for cluster: {}", cluster_name);
    }

    let mut commands = vec![];
    let pp = PatchParams::apply(MANAGER_NAME);

    for computer in computers_for_cluster {
        // TODO: use label selectors
        if !computer.metadata.owner_references.as_ref().is_some_and(|owners| owners.iter().any(|o| Some(o.uid.as_str()) == cluster.metadata.uid.as_deref())) {
            // Skip computers not owned by this cluster
            continue;
        }

        if computer.status.as_ref().map(|stat| &stat.state) != Some(&computer.spec.state) {
            commands.push(Command::Wake { computer_id: computer.spec.id.clone() });
            continue;
        }

        if let Some(status) = &computer.status {
            let is_online = status
                .last_heartbeat_unix_sec
                .is_some_and(|t| t >= (chrono::Utc::now().timestamp() - 300));

            if status.online != is_online {
                // Computer hasn't sent a heartbeat in the last 5 minutes, consider it offline
                // Optionally, send a command to check its status or take other actions
                computers
                    .patch_status(
                        computer.metadata.name.as_deref().unwrap(),
                        &pp,
                        &Patch::Apply(json!({
                            "status": {
                                "online": is_online,
                            }
                        })),
                    )
                    .await?;

                if !is_online {
                    commands.push(Command::Wake { computer_id: computer.spec.id.clone() });
                }
            }
        }
    }

    Ok(commands)
}

pub fn error_policy(_object: Arc<Cluster>, _error: &Error, _context: Arc<ReconcilerCtx>) -> Action {
    Action::requeue(Duration::from_secs(10))
}
