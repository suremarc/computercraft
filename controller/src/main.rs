use kube::Client;
use rocket::launch;

#[launch]
async fn rocket() -> _ {
    let client = Client::try_default().await.expect("connect to k8s");

    controller::rocket(client)
}
