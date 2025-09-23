use std::{sync::Arc, time::Duration};

use futures::Stream;
use k8s_openapi::{
    api::{
        apps::v1::Deployment,
        core::v1::{ConfigMap, Service, ServiceSpec},
    },
    apimachinery::pkg::util::intstr::IntOrString,
};
use kcr_gateway_networking_k8s_io::v1::httproutes::{
    HTTPRoute, HTTPRouteParentRefs, HTTPRouteRules, HTTPRouteRulesBackendRefs,
    HTTPRouteRulesFilters, HTTPRouteRulesFiltersRequestRedirect,
    HTTPRouteRulesFiltersRequestRedirectPath, HTTPRouteRulesFiltersRequestRedirectPathType,
    HTTPRouteRulesFiltersType, HTTPRouteRulesMatches, HTTPRouteRulesMatchesPath, HTTPRouteSpec,
};
use kube::{
    Api, Client, Resource,
    api::{ObjectMeta, Patch, PatchParams},
    runtime::{
        Controller,
        controller::{Action, Error as ControllerError},
        reflector::ObjectRef,
        watcher,
    },
};
use tracing::{Level, instrument};

use crate::{
    Error, Result,
    api::{ComputerGateway, RednetGatewayConfigMapData},
    reconcilers::owner_ref_from_object_ref,
};

const MANAGER_NAME: &str = "cc-gateway-controller";

struct ReconcilerCtx {
    client: Client,
    controller_namespace: String,
}

pub fn control_loop(
    client: Client,
    controller_namespace: String,
) -> impl Stream<
    Item = Result<(ObjectRef<ComputerGateway>, Action), ControllerError<Error, watcher::Error>>,
> {
    let gateways = Api::<ComputerGateway>::all(client.clone());
    let httproutes = Api::<HTTPRoute>::all(client.clone());
    let configmaps = Api::<ConfigMap>::all(client.clone());
    let deployments = Api::<Deployment>::all(client.clone());

    let context = Arc::new(ReconcilerCtx {
        client: client.clone(),
        controller_namespace,
    });

    Controller::new(gateways, watcher::Config::default())
        .owns(httproutes, watcher::Config::default())
        .owns(configmaps, watcher::Config::default())
        .owns(deployments, watcher::Config::default())
        .shutdown_on_signal()
        .run(reconcile, error_policy, context)
}

#[instrument(level = Level::DEBUG, skip(context))]
async fn reconcile(gateway: Arc<ComputerGateway>, context: Arc<ReconcilerCtx>) -> Result<Action> {
    tracing::info!("Reconciling...");

    create_gateway_hub(
        &context.client,
        &gateway,
        context.controller_namespace.clone(),
    )
    .await?;

    Ok(Action::requeue(Duration::from_secs(300)))
}

#[instrument(level = Level::DEBUG, skip(client))]
async fn create_gateway_hub(
    client: &Client,
    gateway: &ComputerGateway,
    controller_namespace: String,
) -> Result<()> {
    let gateway_namespace = gateway.metadata.namespace.as_deref().unwrap();
    let gateway_name = gateway.metadata.name.as_deref().unwrap();

    let deployment_name = format!("rednet-gateway-{}", gateway_name);

    let configmaps = Api::<ConfigMap>::namespaced(client.clone(), gateway_namespace);
    let deployments = Api::<Deployment>::namespaced(client.clone(), gateway_namespace);
    let services = Api::<Service>::namespaced(client.clone(), gateway_namespace);
    let routes = Api::<HTTPRoute>::namespaced(client.clone(), gateway_namespace);

    let pp = PatchParams::apply(MANAGER_NAME);

    configmaps
        .patch(
            &deployment_name,
            &pp,
            &Patch::Apply(ConfigMap {
                metadata: ObjectMeta {
                    name: Some(deployment_name.clone()),
                    namespace: Some(gateway_namespace.to_string()),
                    owner_references: Some(vec![owner_ref_from_object_ref(
                        &gateway.object_ref(&()),
                    )?]),
                    ..Default::default()
                },
                data: Some(
                    [(
                        "rednet".to_string(),
                        serde_yaml_ng::to_string(&RednetGatewayConfigMapData {
                            routes: gateway.spec.routes.clone(),
                        })?,
                    )]
                    .into(),
                ),
                ..Default::default()
            }),
        )
        .await?;

    deployments.patch(&deployment_name, &pp, &Patch::Apply(Deployment {
        metadata: ObjectMeta {
            name: Some(deployment_name.clone()),
            namespace: Some(gateway_namespace.to_string()),
            owner_references: Some(vec![owner_ref_from_object_ref(&gateway.object_ref(&()))?]),
            ..Default::default()
        },
        spec: Some(k8s_openapi::api::apps::v1::DeploymentSpec {
            replicas: Some(1),
            selector: k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector {
                match_labels: Some(
                    [("app".to_string(), deployment_name.clone())]
                        .into(),
                ),
                ..Default::default()
            },
            template: k8s_openapi::api::core::v1::PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(
                        [("app".to_string(), deployment_name.clone())]
                            .into(),
                    ),
                    ..Default::default()
                }),
                spec: Some(k8s_openapi::api::core::v1::PodSpec {
                    containers: vec![
                        k8s_openapi::api::core::v1::Container {
                            name: "rednet-gateway".to_string(),
                            // TODO: use correct version
                            image: Some(std::env::var("GATEWAY_IMAGE").unwrap_or_else(|_| "registry.digitalocean.com/suremarc/computercraft-gateway:latest".to_string())),
                            env: Some(vec![
                                k8s_openapi::api::core::v1::EnvVar {
                                    name: "ROCKET_REDNET".to_string(),
                                    value: Some("/etc/config/rednet".to_string()),
                                    ..Default::default()
                                },
                                k8s_openapi::api::core::v1::EnvVar {
                                    name: "ROCKET_ADDRESS".to_string(),
                                    value: Some("0.0.0.0".to_string()),
                                    ..Default::default()
                                },
                            ]),
                            volume_mounts: Some(vec![
                                k8s_openapi::api::core::v1::VolumeMount {
                                    name: "config".to_string(),
                                    mount_path: "/etc/config".to_string(),
                                    ..Default::default()
                                }
                            ]),
                            ..Default::default()
                        }
                    ],
                    volumes: Some(vec![
                        k8s_openapi::api::core::v1::Volume {
                            name: "config".to_string(),
                            config_map: Some(k8s_openapi::api::core::v1::ConfigMapVolumeSource {
                                name: deployment_name.clone(),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }
                    ]),
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    })).await?;

    services
        .patch(
            &deployment_name,
            &pp,
            &Patch::Apply(Service {
                metadata: ObjectMeta {
                    name: Some(deployment_name.clone()),
                    namespace: Some(gateway_namespace.to_string()),
                    owner_references: Some(vec![owner_ref_from_object_ref(
                        &gateway.object_ref(&()),
                    )?]),
                    ..Default::default()
                },
                spec: Some(ServiceSpec {
                    selector: Some([("app".to_string(), deployment_name.clone())].into()),
                    ports: Some(vec![k8s_openapi::api::core::v1::ServicePort {
                        port: 8000,
                        target_port: Some(IntOrString::Int(8000)),
                        ..Default::default()
                    }]),
                    type_: Some("ClusterIP".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        )
        .await?;

    routes
        .patch(
            &deployment_name,
            &pp,
            &Patch::Apply(HTTPRoute {
                metadata: ObjectMeta {
                    name: Some(deployment_name.clone()),
                    namespace: Some(gateway_namespace.to_string()),
                    owner_references: Some(vec![owner_ref_from_object_ref(
                        &gateway.object_ref(&()),
                    )?]),
                    ..Default::default()
                },
                spec: HTTPRouteSpec {
                    parent_refs: Some(vec![HTTPRouteParentRefs {
                        name: "cc-web-gateway".to_string(),
                        namespace: Some(controller_namespace.clone()),
                        section_name: Some("cc-web-gateway".to_string()),
                        ..Default::default()
                    }]),
                    rules: Some(vec![HTTPRouteRules {
                        matches: Some(vec![HTTPRouteRulesMatches {
                            path: Some(HTTPRouteRulesMatchesPath {
                                value: Some(format!("/{gateway_name}")),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }]),
                        filters: Some(vec![HTTPRouteRulesFilters {
                            r#type: HTTPRouteRulesFiltersType::RequestRedirect,
                            request_redirect: Some(HTTPRouteRulesFiltersRequestRedirect {
                                path: Some(HTTPRouteRulesFiltersRequestRedirectPath {
                                    r#type: HTTPRouteRulesFiltersRequestRedirectPathType::ReplacePrefixMatch,
                                    replace_prefix_match: Some("/".to_string()),
                                    replace_full_path: None,
                                }),
                                status_code: Some(302),
                                ..Default::default()
                            }),
                            extension_ref: None,
                            request_header_modifier: None,
                            request_mirror: None,
                            response_header_modifier: None,
                            url_rewrite: None,
                        }]),
                        backend_refs: Some(vec![HTTPRouteRulesBackendRefs {
                            name: deployment_name.clone(),
                            port: Some(8000),
                            ..Default::default()
                        }]),
                        ..Default::default()
                    }]),
                    ..Default::default()
                },
                ..Default::default()
            }),
        )
        .await?;

    Ok(())
}

fn error_policy(
    _object: Arc<ComputerGateway>,
    _error: &Error,
    _context: Arc<ReconcilerCtx>,
) -> Action {
    Action::requeue(Duration::from_secs(10))
}
