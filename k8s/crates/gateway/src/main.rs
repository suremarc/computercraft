use std::{collections::HashMap, path::PathBuf, pin::Pin, sync::Arc, time::Duration};

use anyhow::Context;
use dashmap::DashMap;
use pin_project::{pin_project, pinned_drop};
use rand::Rng;
use rocket::{
    Data, Request, Response, Route, State,
    data::ByteUnit,
    fairing::AdHoc,
    futures::{
        SinkExt, StreamExt,
        channel::{
            mpsc,
            oneshot::{self, Canceled},
        },
    },
    get,
    http::{Method, Status, ext::IntoOwned, uri::Origin},
    launch,
    outcome::Outcome,
    request::{self, FromRequest},
    response::Responder,
    route::Handler,
    routes,
};
use rocket_ws::Message;
use serde::{Deserialize, Serialize};
use tokio::time::timeout;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]

struct GatewayConfig {
    #[serde(default = "default_gateway_timeout")]
    gateway_timeout: u32,
    rednet: PathBuf,
}

fn default_gateway_timeout() -> u32 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RednetConfig {
    routes: Vec<HttpOverRednetRoute>,
}

#[launch]
async fn rocket() -> _ {
    let server = Arc::<Server>::default();

    rocket::build()
        .attach(AdHoc::config::<GatewayConfig>())
        .manage(Arc::clone(&server))
        .mount("/link", routes![listen])
        .mount(
            "/gateway",
            vec![
                Method::Get,
                Method::Put,
                Method::Post,
                Method::Delete,
                Method::Patch,
            ]
            .into_iter()
            .map(|method| {
                Route::new(
                    method,
                    "/<path..>?<query..>",
                    GatewayHandler {
                        server: Arc::clone(&server),
                    },
                )
            })
            .collect::<Vec<_>>(),
        )
}

// HTTP over Rednet over WebSocket

type ComputerId = String;

#[rocket::async_trait]
impl<'r> FromRequest<'r> for RednetConfig {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> request::Outcome<Self, Self::Error> {
        let gateway_config = State::<GatewayConfig>::get(request.rocket()).unwrap();

        let result = tokio::fs::read_to_string(&gateway_config.rednet)
            .await
            .context("load rednet config")
            .and_then(|data: String| {
                serde_yaml_ng::from_str(&data).context("Failed to parse rednet config")
            });

        let rednet = match result {
            Ok(config) => config,
            Err(e) => {
                rocket::error!("Failed to load rednet config: {e}");
                return Outcome::Error((Status::BadGateway, ()));
            }
        };

        Outcome::Success(rednet)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RednetRpcMessage<T> {
    dest: RednetRpcDestination,
    #[serde(rename = "requestID")]
    request_id: Uuid,
    payload: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HttpRequest {
    method: Method,
    uri: Origin<'static>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    headers: HashMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    body: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for HttpRequest {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> request::Outcome<HttpRequest, Self::Error> {
        let method = request.method();
        let uri = request.uri().clone().into_owned();
        let headers = request.headers().iter().fold(
            HashMap::<String, Vec<String>>::new(),
            |mut acc, header| {
                acc.entry(header.name().to_string())
                    .or_default()
                    .push(header.value().to_string());
                acc
            },
        );

        Outcome::Success(HttpRequest {
            method,
            uri,
            headers,
            body: String::new(), // Placeholder, body will be filled in later
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HttpResponse {
    status: Status,
    #[serde(default)]
    headers: HashMap<String, Vec<String>>,
    #[serde(default)]
    body: String,
}

impl<'r, 'o: 'r> Responder<'r, 'o> for HttpResponse {
    fn respond_to(self, _request: &'r Request<'_>) -> rocket::response::Result<'o> {
        let mut builder = Response::build();
        builder.status(self.status).sized_body(
            self.body.len(),
            std::io::Cursor::new(self.body.into_bytes()),
        );

        for (header_name, header_values) in self.headers {
            for header_value in header_values {
                builder.raw_header(header_name.clone(), header_value);
            }
        }

        builder.ok()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum RednetRpcDestination {
    Anycast {
        protocol: String,
    },
    Computer {
        id: ComputerId,
        protocol: Option<String>,
    },
    Host {
        protocol: String,
        host: String,
    },
}

#[derive(Debug, Default)]
struct Server {
    listeners: DashMap<ComputerId, mpsc::Sender<RednetRpcMessage<HttpRequest>>>,
    in_flight_requests: DashMap<Uuid, oneshot::Sender<HttpResponse>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HttpOverRednetRoute {
    prefix: PathBuf,
    backend: RednetRpcDestination,
}

impl HttpOverRednetRoute {
    fn check(&self, req: &HttpRequest) -> bool {
        match self.prefix.to_str() {
            Some(prefix_str) => req.uri.path().starts_with(prefix_str),
            None => false,
        }
    }
}

#[derive(Clone)]
struct GatewayHandler {
    server: Arc<Server>,
}

#[rocket::async_trait]
impl Handler for GatewayHandler {
    async fn handle<'r>(
        &self,
        request: &'r Request<'_>,
        data: Data<'r>,
    ) -> rocket::route::Outcome<'r> {
        let gateway_config = State::<GatewayConfig>::get(request.rocket()).unwrap();

        let rednet = match RednetConfig::from_request(request).await {
            Outcome::Success(cfg) => cfg,
            Outcome::Error((status, ())) => {
                rocket::error!("Failed to get rednet config during request");
                return Outcome::Error(status);
            }
            Outcome::Forward(status) => return Outcome::Forward((data, status)),
        };

        let mut http_request = HttpRequest::from_request(request).await.unwrap();

        http_request.uri = match http_request
            .uri
            .map_path(|p| p.strip_prefix("/gateway").unwrap_or(p))
        {
            Some(u) => u,
            None => {
                rocket::error!(
                    "Unexpected error stripping /gateway prefix from path: {}",
                    http_request.uri
                );
                return Outcome::Error(Status::InternalServerError);
            }
        };

        let dest = match rednet
            .routes
            .iter()
            .find_map(|route| route.check(&http_request).then_some(route.backend.clone()))
        {
            None => return Outcome::Error(Status::NotFound),
            Some(dest) => dest,
        };

        http_request.body = match data.open(ByteUnit::Mebibyte(1)).into_string().await {
            Ok(body) if body.is_complete() => body.into_inner(),
            _ => {
                rocket::error!("Incomplete body from client");
                return Outcome::Error(Status::InternalServerError);
            }
        };

        let request_id = Uuid::new_v4();

        let rx = match self
            .server
            .new_request(RednetRpcMessage {
                dest,
                request_id,
                payload: http_request,
            })
            .await
        {
            Err(status) => return Outcome::Error(status),
            Ok(rx) => rx,
        };

        let resp = match timeout(
            Duration::from_secs(gateway_config.gateway_timeout as u64),
            rx,
        )
        .await
        {
            Err(_) => return Outcome::Error(Status::GatewayTimeout),
            Ok(Err(_)) => return Outcome::Error(Status::BadGateway),
            Ok(Ok(msg)) => msg,
        };

        Outcome::Success(resp.respond_to(request).unwrap())
    }
}

impl Server {
    async fn new_request(
        self: &Arc<Self>,
        message: RednetRpcMessage<HttpRequest>,
    ) -> Result<RednetRpcReceiver, Status> {
        let (tx, rx) = oneshot::channel();

        // Get a random listener
        let mut listeners = self.listeners.iter().map(|r| r.clone()).collect::<Vec<_>>();
        if listeners.is_empty() {
            rocket::error!("No listeners available for rednet request");
            return Err(Status::BadGateway);
        }

        let num_listeners = listeners.len();
        let listener = listeners
            .get_mut(rand::rng().random_range(0..num_listeners))
            .ok_or(Status::InternalServerError)
            .inspect_err(|_| {
                rocket::error!("No listeners available for rednet request (listener membership changed mid-request");
            })?;

        if let Err(_e) = listener.send(message.clone()).await {
            rocket::error!("Failed to send message to listener (pipe closed)");
            return Err(Status::InternalServerError);
        }

        self.in_flight_requests.insert(message.request_id, tx);

        Ok(RednetRpcReceiver {
            server: Arc::clone(self),
            request_id: message.request_id,
            receiver: rx,
        })
    }

    fn cancel_request(&self, request_id: &Uuid) {
        self.in_flight_requests.remove(request_id);
    }
}

#[get("/<id>")]
async fn listen<'a>(
    ws: rocket_ws::WebSocket,
    id: &'a str,
    server: &'a State<Arc<Server>>,
) -> Result<rocket_ws::Stream!['a], Status> {
    let (tx, mut rx) = mpsc::channel(1000);
    server.listeners.insert(id.to_string(), tx);

    Ok(ws.stream(move |mut ws| {
        rocket::async_stream::try_stream! {
            scopeguard::defer!(
                rocket::info!("Listener {} disconnected", id);
                server.listeners.remove(id);
            );

            loop {
                tokio::select! {
                    res = rx.next() => {
                        let msg = match res {
                            None => break,
                            Some(msg) => msg,
                        };

                        yield Message::Text(serde_json::to_string(&msg).unwrap());
                    },
                    res = ws.next() =>  match res {
                        Some(Ok(Message::Text(text))) => {
                            match serde_json::from_str::<RednetRpcMessage<HttpResponse>>(&text) {
                                Ok(msg) => {
                                    handle_response(server, msg).await;
                                }
                                Err(e) => {
                                    rocket::error!("Failed to deserialize message: {}", e);
                                    break;
                                }
                            }
                        },
                        Some(Ok(Message::Ping(payload))) => {
                            yield Message::Pong(payload);
                        }
                        Some(Err(_)) => {
                            break;
                        },
                        _ => break,
                    }
                }
            }
        }
    }))
}

async fn handle_response(server: &Server, message: RednetRpcMessage<HttpResponse>) {
    match server.in_flight_requests.remove(&message.request_id) {
        Some((_, tx)) => {
            let _ = tx.send(message.payload);
        }
        None => {
            rocket::warn!(
                "Received response for unknown request ID: {}",
                message.request_id
            );
        }
    }
}

#[pin_project(PinnedDrop)]
struct RednetRpcReceiver {
    server: Arc<Server>,
    request_id: Uuid,
    #[pin]
    receiver: oneshot::Receiver<HttpResponse>,
}

impl Future for RednetRpcReceiver {
    type Output = Result<HttpResponse, Canceled>;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.project().receiver.poll(cx)
    }
}

#[pinned_drop]
impl PinnedDrop for RednetRpcReceiver {
    fn drop(self: Pin<&mut Self>) {
        self.server.cancel_request(&self.request_id);
    }
}
