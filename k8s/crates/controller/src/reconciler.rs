use std::{sync::Arc, time::Duration};

use k8s_openapi::{
    api::{
        core::v1::{ObjectReference, Secret, ServiceAccount},
        rbac::v1::{PolicyRule, Role, RoleBinding, RoleRef, Subject},
    },
    apimachinery::pkg::apis::meta::v1::OwnerReference,
};
use kube::{
    Api, Client, Resource,
    api::{ListParams, ObjectMeta, Patch, PatchParams},
    runtime::controller::Action,
};
use serde_json::json;
use tracing::{Level, instrument};

use crate::{
    Error, GatewayCommand, Result,
    api::{Computer, ComputerCluster},
};

const MANAGER_NAME: &str = "computercraft-controller";

pub struct ReconcilerCtx {
    pub client: Client,
}

#[instrument(level = Level::DEBUG, skip(context))]
pub async fn reconcile(
    cluster: Arc<ComputerCluster>,
    context: Arc<ReconcilerCtx>,
) -> Result<Action> {
    tracing::info!("Reconciling...");

    let cluster_namespace = cluster.metadata.namespace.as_deref().unwrap();
    let _cluster_name = cluster.metadata.name.as_deref().unwrap();

    create_cluster_rbac(&context.client, cluster.as_ref()).await?;

    let computers = Api::<Computer>::namespaced(context.client.clone(), cluster_namespace);

    let commands = compute_cluster_diff_and_set_statuses(&computers, cluster.as_ref()).await?;
    if commands.is_empty() {
        // The cluster is in a good state, check again in 5 minutes
        return Ok(Action::requeue(Duration::from_secs(300)));
    }

    // TODO: send commands to new gateway
    // context
    //     .c2_server
    //     .sender(cluster_namespace, cluster_name)
    //     .send(commands)?;

    // Check again in 10 seconds
    Ok(Action::requeue(Duration::from_secs(10)))
}

/// Create a service account for computers in this cluster if it doesn't already exist
#[instrument(level = Level::DEBUG, skip(client))]
async fn create_cluster_rbac(client: &Client, cluster: &ComputerCluster) -> Result<()> {
    let cluster_namespace = cluster.metadata.namespace.as_deref().unwrap();
    let cluster_name = cluster.metadata.name.as_deref().unwrap();

    let service_accounts = Api::<ServiceAccount>::namespaced(client.clone(), cluster_namespace);
    let roles = Api::<Role>::namespaced(client.clone(), cluster_namespace);
    let role_bindings = Api::<RoleBinding>::namespaced(client.clone(), cluster_namespace);
    let secrets = Api::<Secret>::namespaced(client.clone(), cluster_namespace);

    let pp = PatchParams::apply(MANAGER_NAME);

    let name = format!("computer-{}", cluster_name);

    let cluster_as_owner_ref = owner_ref_from_object_ref(&cluster.object_ref(&()))?;

    roles
        .patch(
            &name,
            &pp,
            &Patch::Apply(Role {
                metadata: ObjectMeta {
                    name: Some(name.clone()),
                    owner_references: Some(vec![cluster_as_owner_ref.clone()]),
                    ..Default::default()
                },
                rules: Some(vec![
                    PolicyRule {
                        api_groups: Some(vec!["sms.dev".to_string()]),
                        resources: Some(vec!["computers".to_string()]),
                        verbs: vec!["create".to_string(), "delete".to_string()],
                        ..Default::default()
                    },
                    PolicyRule {
                        api_groups: Some(vec!["sms.dev".to_string()]),
                        resources: Some(vec!["computers/status".to_string()]),
                        verbs: vec!["update".to_string(), "patch".to_string()],
                        ..Default::default()
                    },
                ]),
            }),
        )
        .await?;

    service_accounts
        .patch(
            &name,
            &pp,
            &Patch::Apply(ServiceAccount {
                metadata: ObjectMeta {
                    name: Some(name.clone()),
                    owner_references: Some(vec![cluster_as_owner_ref.clone()]),
                    ..Default::default()
                },
                ..Default::default()
            }),
        )
        .await?;

    role_bindings
        .patch(
            &name,
            &pp,
            &Patch::Apply(RoleBinding {
                metadata: ObjectMeta {
                    name: Some(name.clone()),
                    owner_references: Some(vec![cluster_as_owner_ref.clone()]),
                    ..Default::default()
                },
                role_ref: RoleRef {
                    kind: "Role".to_string(),
                    name: name.clone(),
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
                metadata: ObjectMeta {
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
    cluster: &ComputerCluster,
) -> Result<Vec<GatewayCommand>> {
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
            commands.push(GatewayCommand::Wake {
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
                    commands.push(GatewayCommand::Wake {
                        computer_id: computer.spec.id.clone(),
                    });
                }
            }
        }
    }

    Ok(commands)
}

pub fn error_policy(
    _object: Arc<ComputerCluster>,
    _error: &Error,
    _context: Arc<ReconcilerCtx>,
) -> Action {
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
