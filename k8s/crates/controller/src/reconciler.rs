use std::{sync::Arc, time::Duration};

use k8s_openapi::{
    api::{
        apps::v1::Deployment, core::v1::{ConfigMap, ObjectReference, Secret, ServiceAccount}, rbac::v1::{PolicyRule, Role, RoleBinding, RoleRef, Subject}
    },
    apimachinery::pkg::apis::meta::v1::OwnerReference,
};
use kcr_gateway_networking_k8s_io::v1::gateways::{Gateway, GatewayListeners, GatewayListenersAllowedRoutes, GatewayListenersAllowedRoutesNamespaces, GatewayListenersAllowedRoutesNamespacesFrom, GatewaySpec};
use kube::{
    Api, Client, Resource,
    api::{ListParams, ObjectMeta, Patch, PatchParams},
    runtime::controller::Action,
};
use serde_json::json;
use tracing::{Level, instrument};

use crate::{
    Error, GatewayCommand, Result,
    api::{Computer, ComputerCluster, ComputerGatewayLink},
};

const MANAGER_NAME: &str = "computercraft-controller";

pub struct ReconcilerCtx {
    pub client: Client,
    pub namespace: String,
}

#[instrument(level = Level::DEBUG, skip(context))]
pub async fn reconcile(
    cluster: Arc<ComputerCluster>,
    context: Arc<ReconcilerCtx>,
) -> Result<Action> {
    tracing::info!("Reconciling...");

    let cluster_namespace = cluster.metadata.namespace.as_deref().unwrap();

    create_cluster_rbac(&context.client, cluster.as_ref()).await?;

    let computers = Api::<Computer>::namespaced(context.client.clone(), cluster_namespace);

    if let Err(e) = create_gateways(&context.client, &cluster, &context.namespace).await {
        tracing::error!("Failed to create gateway: {:?}", e);
    }

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

async fn create_gateways(client: &Client, cluster: &ComputerCluster, controller_namespace: &str) -> Result<()> {
    let gateways = Api::<Gateway>::namespaced(client.clone(), controller_namespace);

    let pp = PatchParams::apply(MANAGER_NAME);

    const GATEWAY_NAME: &str = "cc-web-gateway";

    gateways.patch(GATEWAY_NAME, &pp, &Patch::Apply(Gateway {
        metadata: ObjectMeta {
            name: Some(GATEWAY_NAME.to_string()),
            namespace: Some(controller_namespace.to_string()),
            ..Default::default()
        },
        spec: GatewaySpec {
            gateway_class_name: "cilium".to_string(),
            listeners: vec![
                GatewayListeners {
                    protocol: "HTTP".to_string(),
                    port: 80,
                    name: GATEWAY_NAME.to_string(),
                    allowed_routes: Some(GatewayListenersAllowedRoutes {
                        namespaces: Some(GatewayListenersAllowedRoutesNamespaces {
                            from: Some(GatewayListenersAllowedRoutesNamespacesFrom::All),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
            ],
            ..Default::default()
        },
        ..Default::default()
    })).await?;

    // Create a rednet gateway for this cluster

    let cluster_namespace = cluster.metadata.namespace.as_deref().unwrap();
    let cluster_name = cluster.metadata.name.as_deref().unwrap();

    let configmaps = Api::<ConfigMap>::namespaced(client.clone(), &cluster_namespace)
    let deployments = Api::<Deployment>::namespaced(client.clone(), &cluster_namespace);

    let rednet_gateway_name = format!("rednet-gateway-{}", cluster_name);

    configmaps.patch(&rednet_gateway_name, &pp, &Patch::Apply(ConfigMap {
        metadata: ObjectMeta {
            name: Some(rednet_gateway_name.clone()),
            namespace: Some(cluster_namespace.to_string()),
            ..Default::default()
        },
        data: Some(
            [
                ("CLUSTER_NAMESPACE".to_string(), cluster_namespace.to_string()),
                ("CLUSTER_NAME".to_string(), cluster_name.to_string()),
            ]
            .into(),
        ),
        ..Default::default()
    })).await?;

    deployments.patch(&rednet_gateway_name, &pp, &Patch::Apply(Deployment {
        metadata: ObjectMeta {
            name: Some(rednet_gateway_name.clone()),
            namespace: Some(cluster_namespace.to_string()),
            ..Default::default()
        },
        spec: Some(k8s_openapi::api::apps::v1::DeploymentSpec {
            replicas: Some(1),
            selector: k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector {
                match_labels: Some(
                    [("app".to_string(), "rednet-gateway".to_string())]
                        .into(),
                ),
                ..Default::default()
            },
            template: k8s_openapi::api::core::v1::PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(
                        [("app".to_string(), "rednet-gateway".to_string())]
                            .into(),
                    ),
                    ..Default::default()
                }),
                spec: Some(k8s_openapi::api::core::v1::PodSpec {
                    service_account_name: Some(format!("computer-{}", cluster_name)),
                    containers: vec![
                        k8s_openapi::api::core::v1::Container {
                            name: "rednet-gateway".to_string(),
                            image: Some("ghcr.io/suremarc/computercraft-rednet-gateway:latest".to_string()),
                            env: Some(vec![
                                k8s_openapi::api::core::v1::EnvVar {
                                    name: "CLUSTER_NAMESPACE".to_string(),
                                    value: Some(cluster_namespace.to_string()),
                                    ..Default::default()
                                },
                                k8s_openapi::api::core::v1::EnvVar {
                                    name: "CLUSTER_NAME".to_string(),
                                    value: Some(cluster_name.to_string()),
                                    ..Default::default()
                                },
                            ]),
                            ..Default::default()
                        }
                    ],
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    })).await?;

    Ok(())
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
