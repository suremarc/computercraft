use std::sync::Arc;

use clap::{Parser, Subcommand};
use futures::StreamExt;
use kube::{
    Api, Client, CustomResourceExt,
    runtime::{Controller, watcher},
};

use controller::{
    api::{Computer, ComputerCluster, ComputerGateway, ComputerGatewayLink, HttpOverRednetRoute},
    reconciler::{self, ReconcilerCtx},
};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Clone, Parser)]
#[command(version, about)]
struct Cli {
    #[arg(short, long, env = "KUBE_NAMESPACE")]
    namespace: String,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Clone, Subcommand)]
enum Commands {
    /// Run the controller reconciliation loop
    Reconcile,
    /// Output K8s manifest for a given CRD resource
    #[command(subcommand)]
    CrdManifest(Crd),
}

#[derive(Debug, Clone, Subcommand)]
enum Crd {
    Cluster,
    Computer,
    GatewayLink,
    HttpOverRednetRoute,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(true)
                .with_file(true)
                .with_line_number(true),
        )
        .with(EnvFilter::from_default_env())
        .try_init()?;

    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Reconcile) => run_controller(cli.namespace).await?,
        Some(Commands::CrdManifest(crd)) => {
            let crd = match crd {
                Crd::Cluster => ComputerCluster::crd(),
                Crd::Computer => Computer::crd(),
                Crd::GatewayLink => ComputerGatewayLink::crd(),
                Crd::HttpOverRednetRoute => HttpOverRednetRoute::crd(),
            };

            println!("{}", serde_yaml_ng::to_string(&crd)?);
        }
        None => {}
    }

    Ok(())
}

async fn run_controller(controller_namespace: String) -> anyhow::Result<()> {
    let client = Client::try_default().await.expect("connect to k8s");

    let ctx = Arc::new(ReconcilerCtx {
        client: client.clone(),
        namespace: controller_namespace,
    });

    let clusters = Api::<ComputerCluster>::all(client.clone());
    let computers = Api::<Computer>::all(client.clone());
    let gateway_links = Api::<ComputerGatewayLink>::all(client.clone());
    let http_routes = Api::<HttpOverRednetRoute>::all(client.clone());

    Controller::new(clusters, watcher::Config::default())
        // TODO: use label selectors to only watch objects we care about
        .owns(computers, watcher::Config::default())
        .owns(gateway_links, watcher::Config::default())
        .owns(http_routes, watcher::Config::default())
        .shutdown_on_signal()
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
        })
        .await;

    tracing::info!("controller terminated");
    Ok(())
}
