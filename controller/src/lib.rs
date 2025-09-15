/// K8s API objects
pub mod api;

/// C2 (Command & Control) server
pub mod c2;

/// K8s reconciliation logic
pub mod reconciler;

use std::sync::Arc;

use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{
    Api, CustomResourceExt,
    runtime::{Controller, watcher},
};
use rocket::{
    Build, Rocket,
    fairing::AdHoc,
    futures::{StreamExt, channel::mpsc},
    get, routes,
    serde::json::Json,
};
use thiserror::Error;
use tokio::sync::watch::error::SendError;

use crate::{
    api::{Cluster, Computer},
    c2::{C2Server, Command},
    reconciler::ReconcilerCtx,
};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Kube error: {0}")]
    Kube(#[from] kube::Error),
    #[error("No peers available for cluster: {0}")]
    ClusterUnavailable(#[from] SendError<Vec<Command>>),
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn rocket(client: kube::Client) -> Rocket<Build> {
    let (reconciler_tx, reconciler_rx) = mpsc::channel(100);

    let ctx = Arc::new(ReconcilerCtx {
        client: client.clone(),
        c2_server: Arc::new(C2Server::new(client.clone(), reconciler_tx)),
    });

    let clusters = Api::<Cluster>::all(client.clone());
    let computers = Api::<Computer>::all(client.clone());

    rocket::build()
        .manage(Arc::clone(&ctx.c2_server))
        .attach(AdHoc::on_liftoff("reconciler", |_| {
            Box::pin(async move {
                tokio::spawn(
                    Controller::new(clusters, watcher::Config::default())
                        .owns(computers, watcher::Config::default())
                        .shutdown_on_signal()
                        .reconcile_on(reconciler_rx)
                        .run(
                            reconciler::reconcile,
                            reconciler::error_policy,
                            Arc::clone(&ctx),
                        )
                        .for_each(|res| async move {
                            match res {
                                Ok(o) => tracing::info!("Reconciled {:?}", o),
                                Err(e) => tracing::error!("Reconcile failed: {:?}", e),
                            }
                        }),
                );
            })
        }))
        .mount("/", routes![c2::bridge])
        .mount("/crd", routes![cluster_crd, computer_crd])
}

// crd routes

#[get("/cluster")]
fn cluster_crd() -> Json<CustomResourceDefinition> {
    Json(Cluster::crd())
}

#[get("/computer")]
fn computer_crd() -> Json<CustomResourceDefinition> {
    Json(Computer::crd())
}
