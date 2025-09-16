use std::{sync::Arc, time::Duration};

use k8s_openapi::{
    api::{
        core::v1::{ObjectReference, Secret, ServiceAccount},
        rbac::v1::{ClusterRoleBinding, RoleRef, Subject},
    },
    apimachinery::pkg::apis::meta::v1::OwnerReference,
};
use kube::{
    Api, Client, Resource,
    api::{ListParams, Patch, PatchParams},
    runtime::controller::Action,
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

#[instrument(level = Level::DEBUG, skip(context))]
pub async fn reconcile(cluster: Arc<Cluster>, context: Arc<ReconcilerCtx>) -> Result<Action> {
    let cluster_namespace = cluster.metadata.namespace.as_deref().unwrap();
    let cluster_name = cluster.metadata.name.as_deref().unwrap();

    create_cluster_rbac(&context.client, cluster.as_ref()).await?;

    let computers = Api::<Computer>::namespaced(context.client.clone(), cluster_namespace);

    let commands = compute_cluster_diff_and_set_statuses(&computers, cluster.as_ref()).await?;
    if commands.is_empty() {
        // The cluster is in a good state, check again in 5 minutes
        return Ok(Action::requeue(Duration::from_secs(300)));
    }

    context
        .c2_server
        .sender(cluster_namespace, cluster_name)
        .send(commands)?;

    // Check again in 10 seconds
    Ok(Action::requeue(Duration::from_secs(10)))
}

/// Create a service account for computers in this cluster if it doesn't already exist
#[instrument(level = Level::DEBUG, skip(client))]
async fn create_cluster_rbac(client: &Client, cluster: &Cluster) -> Result<()> {
    let cluster_namespace = cluster.metadata.namespace.as_deref().unwrap();
    let cluster_name = cluster.metadata.name.as_deref().unwrap();

    let service_accounts = Api::<ServiceAccount>::namespaced(client.clone(), cluster_namespace);
    let cluster_role_bindings = Api::<ClusterRoleBinding>::all(client.clone()); // ClusterRoleBindings are not namespaced
    let secrets = Api::<Secret>::namespaced(client.clone(), cluster_namespace);

    let pp = PatchParams::apply(MANAGER_NAME);

    let name = format!("computer-{}", cluster_name);

    let cluster_as_owner_ref = owner_ref_from_object_ref(&cluster.object_ref(&()))?;

    service_accounts
        .patch(
            &name,
            &pp,
            &Patch::Apply(ServiceAccount {
                metadata: kube::api::ObjectMeta {
                    name: Some(name.clone()),
                    owner_references: Some(vec![cluster_as_owner_ref.clone()]),
                    ..Default::default()
                },
                ..Default::default()
            }),
        )
        .await?;

    cluster_role_bindings
        .patch(
            &name,
            &pp,
            &Patch::Apply(ClusterRoleBinding {
                metadata: kube::api::ObjectMeta {
                    name: Some(name.clone()),
                    owner_references: Some(vec![cluster_as_owner_ref.clone()]),
                    ..Default::default()
                },
                role_ref: RoleRef {
                    kind: "ClusterRole".to_string(),
                    name: "computer".to_string(),
                    ..Default::default()
                },
                subjects: Some(vec![Subject {
                    kind: "ServiceAccount".to_string(),
                    name: name.clone(),
                    namespace: Some(cluster_namespace.to_string()),
                    ..Default::default()
                }]),
            }),
        )
        .await?;

    secrets
        .patch(
            &name,
            &pp,
            &Patch::Apply(Secret {
                metadata: kube::api::ObjectMeta {
                    name: Some(name.clone()),
                    owner_references: Some(vec![cluster_as_owner_ref]),
                    annotations: Some(
                        [(
                            "kubernetes.io/service-account.name".to_string(),
                            name.clone(),
                        )]
                        .into(),
                    ),
                    ..Default::default()
                },
                type_: Some("kubernetes.io/service-account-token".to_string()),
                ..Default::default()
            }),
        )
        .await?;

    Ok(())
}

async fn compute_cluster_diff_and_set_statuses(
    computers: &Api<Computer>,
    cluster: &Cluster,
) -> Result<Vec<Command>> {
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
        if !computer
            .metadata
            .owner_references
            .as_ref()
            .is_some_and(|owners| {
                owners
                    .iter()
                    .any(|o| Some(o.uid.as_str()) == cluster.metadata.uid.as_deref())
            })
        {
            // Skip computers not owned by this cluster
            continue;
        }

        if computer.status.as_ref().map(|stat| &stat.state) != Some(&computer.spec.state) {
            commands.push(Command::Wake {
                computer_id: computer.spec.id.clone(),
            });
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
                    commands.push(Command::Wake {
                        computer_id: computer.spec.id.clone(),
                    });
                }
            }
        }
    }

    Ok(commands)
}

pub fn error_policy(_object: Arc<Cluster>, _error: &Error, _context: Arc<ReconcilerCtx>) -> Action {
    Action::requeue(Duration::from_secs(10))
}

fn owner_ref_from_object_ref(object_ref: &ObjectReference) -> Result<OwnerReference> {
    Ok(OwnerReference {
        api_version: object_ref
            .api_version
            .clone()
            .ok_or_else(|| Error::MissingField)?,
        kind: object_ref.kind.clone().ok_or_else(|| Error::MissingField)?,
        name: object_ref.name.clone().ok_or_else(|| Error::MissingField)?,
        uid: object_ref.uid.clone().ok_or_else(|| Error::MissingField)?,
        ..Default::default()
    })
}
