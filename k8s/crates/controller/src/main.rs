use clap::{Parser, Subcommand};
use futures::StreamExt;
use kube::{Client, CustomResourceExt};

use controller::{
    api::{Computer, ComputerCluster, ComputerGateway},
    reconcilers,
};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Clone, Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Clone, Subcommand)]
enum Commands {
    /// Run the controller reconciliation loop
    #[command(subcommand)]
    Reconcile(ReconcileTarget),
    /// Output K8s manifest for a given CRD resource
    #[command(subcommand)]
    CrdManifest(Crd),
}

#[derive(Debug, Clone, Subcommand)]
enum ReconcileTarget {
    Clusters,
    Gateways,
}

#[derive(Debug, Clone, Subcommand)]
enum Crd {
    Cluster,
    Computer,
    Gateway,
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
        Some(Commands::Reconcile(target)) => run_controller(target).await?,
        Some(Commands::CrdManifest(crd)) => {
            let crd = match crd {
                Crd::Cluster => ComputerCluster::crd(),
                Crd::Computer => Computer::crd(),
                Crd::Gateway => ComputerGateway::crd(),
            };

            println!("{}", serde_yaml_ng::to_string(&crd)?);
        }
        None => {}
    }

    Ok(())
}

async fn run_controller(target: ReconcileTarget) -> anyhow::Result<()> {
    let client = Client::try_default().await.expect("connect to k8s");

    match target {
        ReconcileTarget::Clusters => {
            reconcilers::cluster::control_loop(client.clone())
                .for_each(|res| async move {
                    match res {
                        Ok(o) => tracing::info!("Reconciled cluster {:?}", o),
                        Err(e) => tracing::error!("Cluster reconcile failed: {:?}", e),
                    }
                })
                .await
        }
        ReconcileTarget::Gateways => {
            reconcilers::gateway::control_loop(client)
                .for_each(|res| async move {
                    match res {
                        Ok(o) => tracing::info!("Reconciled gateway {:?}", o),
                        Err(e) => tracing::error!("Gateway reconcile failed: {:?}", e),
                    }
                })
                .await
        }
    };

    tracing::info!("controller terminated");
    Ok(())
}
