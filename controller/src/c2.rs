use std::sync::Arc;

use dashmap::DashMap;
use kube::{Api, runtime::reflector::ObjectRef};
use rocket::{State, futures::{SinkExt, channel::mpsc}, get, http::Status};
use rocket_ws::Message;
use serde::Serialize;
use tokio::sync::watch::Sender;

use crate::api::Cluster;

/// Each Command is implicitly bound to a Cluster.
#[derive(Debug, Clone, Serialize)]
pub enum Command {
    /// Request the computer to check its desired state and attempt to reconcile itself.
    /// If the computer is offline, it should be woken up if possible.
    Wake { computer_id: String },
}

pub struct C2Server {
    // indexed by namespace + cluster
    cluster_watchers: DashMap<(String, String), Sender<Vec<Command>>>,
    client: kube::Client,
    
    cluster_listener_connected_tx: mpsc::Sender<ObjectRef<Cluster>>,
}

impl C2Server {
    pub fn new(client: kube::Client, tx: mpsc::Sender<ObjectRef<Cluster>>) -> Self {
        Self {
            client,
            cluster_watchers: DashMap::new(),
            cluster_listener_connected_tx: tx,
        }
    }

    pub fn sender(&self, namespace: &str, cluster: &str) -> Sender<Vec<Command>> {
        self.cluster_watchers
            .entry((namespace.to_string(), cluster.to_string()))
            .or_default()
            .clone()
    }
}

#[get("/bridge/<namespace>/<cluster>")]
pub async fn bridge(
    ws: rocket_ws::WebSocket,
    namespace: &str,
    cluster: &str,
    server: &State<Arc<C2Server>>,
) -> Result<rocket_ws::Stream!['static], Status> {
    // First check if the cluster exists

    let clusters = Api::<Cluster>::namespaced(server.client.clone(), namespace);

    match clusters.get(cluster).await {
        Err(kube::Error::Api(e)) if e.code == 404 => return Err(Status::NotFound),
        Err(e) => {
            tracing::error!("Error fetching cluster {}: {:?}", cluster, e);
            return Err(Status::InternalServerError);
        }
        Ok(_) => {}
    }

    let _ = server.cluster_listener_connected_tx.clone().send(ObjectRef::new(cluster).within(namespace)).await.inspect_err(|e| {
        tracing::error!("Failed to notify controller of new listener: {:?}", e);
    });

    let mut recv = server.sender(namespace, cluster).subscribe();

    Ok(ws.stream(move |_ws| {
        rocket::async_stream::try_stream! {
            loop {
                let cmds = recv.borrow_and_update().clone();
                // todo: handle errors properly
                yield Message::Text(serde_json::to_string(&cmds).unwrap());
                recv.changed().await.unwrap();
            }
        }
    }))
}
