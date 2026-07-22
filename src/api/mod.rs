//! Axum REST, SSE, health, and same-origin `WebUI` routes.

mod auth;

use std::{cmp::Ordering, convert::Infallible, str::FromStr as _, time::Duration};

use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query, Request, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response, Sse, sse::Event},
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use tokio_stream::{StreamExt as _, wrappers::BroadcastStream};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use crate::{
    domain::{NodeId, NodeSummary, OperationId, TestState},
    service::{AppState, ServiceError},
};

use self::auth::AuthManager;

/// Combined state for API handlers and LAN authentication.
#[derive(Clone)]
pub struct ApiState {
    app: AppState,
    auth: AuthManager,
}

/// Stable JSON error envelope.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope<'a> {
    error: ErrorBody<'a>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internalError",
            "内部错误",
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorEnvelope {
                error: ErrorBody {
                    code: self.code,
                    message: &self.message,
                },
            }),
        )
            .into_response()
    }
}

impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        let (status, code) = match error {
            ServiceError::NodeNotFound | ServiceError::OperationNotFound => {
                (StatusCode::NOT_FOUND, "notFound")
            }
            ServiceError::NodeUnavailable => (StatusCode::CONFLICT, "nodeUnavailable"),
            ServiceError::RefreshBusy => (StatusCode::CONFLICT, "refreshBusy"),
            ServiceError::QueueFull => (StatusCode::TOO_MANY_REQUESTS, "testQueueFull"),
            ServiceError::ShuttingDown => (StatusCode::SERVICE_UNAVAILABLE, "shuttingDown"),
            ServiceError::Refresh(_) => (StatusCode::BAD_GATEWAY, "refreshFailed"),
            ServiceError::Worker(_) => (StatusCode::BAD_GATEWAY, "workerFailed"),
            ServiceError::Store(_) => (StatusCode::INTERNAL_SERVER_ERROR, "storageFailed"),
        };
        Self::new(status, code, error.to_string())
    }
}

/// Builds all public and authenticated routes.
pub fn router(app: AppState) -> Router {
    let auth = AuthManager::new(
        app.config().web_credentials.clone(),
        app.config().tls.is_some(),
    );
    let state = ApiState { app, auth };
    let protected = Router::new()
        .route("/nodes", get(list_nodes))
        .route("/nodes/refresh", post(refresh_nodes))
        .route("/connection", put(connect).delete(disconnect))
        .route("/nodes/{node_id}/tests", post(start_test))
        .route("/tests/{operation_id}", get(test_status))
        .route("/status", get(status))
        .route("/events", get(events))
        .route("/auth/session", delete(auth::logout))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));
    let api = Router::new()
        .route("/auth/login", post(auth::login))
        .route("/auth/session", get(auth::session))
        .merge(protected)
        .method_not_allowed_fallback(api_method_not_allowed)
        .fallback(api_not_found);
    let mut router = Router::new()
        .nest("/api/v1", api)
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn(security_headers))
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    let index = state.app.config().web_dist_dir.join("index.html");
    if index.is_file() {
        router = router.fallback_service(
            ServeDir::new(&state.app.config().web_dist_dir).fallback(ServeFile::new(index)),
        );
    } else {
        router = router.fallback(get(missing_webui));
    }
    router
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
        ),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    response
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NodesQuery {
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default = "default_page_size")]
    page_size: usize,
    #[serde(default)]
    search: String,
    #[serde(default)]
    sort: SortKey,
    #[serde(default)]
    order: SortOrder,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
enum SortKey {
    #[default]
    Score,
    Ping,
    Speed,
    Sessions,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SortOrder {
    Asc,
    #[default]
    Desc,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodesPage {
    items: Vec<NodeSummary>,
    page: usize,
    page_size: usize,
    total: usize,
}

async fn list_nodes(
    State(state): State<ApiState>,
    query: Result<Query<NodesQuery>, QueryRejection>,
) -> Result<Json<NodesPage>, ApiError> {
    let Query(query) = query
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalidQuery", "查询参数无效"))?;
    if query.page == 0 || !(1..=200).contains(&query.page_size) || query.search.len() > 256 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalidPagination",
            "分页或搜索参数无效",
        ));
    }
    let snapshot = state.app.nodes().await;
    let mut nodes = snapshot
        .iter()
        .filter(|node| node.matches_search(&query.search))
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| compare_nodes(left, right, query.sort, query.order));
    let total = nodes.len();
    let offset = query.page.saturating_sub(1).saturating_mul(query.page_size);
    let tests = state
        .app
        .latest_tests()
        .await
        .map_err(ServiceError::Store)?;
    let items = nodes
        .into_iter()
        .skip(offset)
        .take(query.page_size)
        .map(|node| node.summary(tests.get(&node.id).cloned()))
        .collect();
    Ok(Json(NodesPage {
        items,
        page: query.page,
        page_size: query.page_size,
        total,
    }))
}

fn compare_nodes(
    left: &crate::domain::VpnNode,
    right: &crate::domain::VpnNode,
    key: SortKey,
    order: SortOrder,
) -> Ordering {
    if matches!(key, SortKey::Ping) {
        match (left.ping_ms, right.ping_ms) {
            (Some(_), None) => return Ordering::Less,
            (None, Some(_)) => return Ordering::Greater,
            _ => {}
        }
    }
    let ordering = match key {
        SortKey::Score => left.score.cmp(&right.score),
        SortKey::Ping => left.ping_ms.cmp(&right.ping_ms),
        SortKey::Speed => left.speed_bps.cmp(&right.speed_bps),
        SortKey::Sessions => left.sessions.cmp(&right.sessions),
    };
    match order {
        SortOrder::Asc => ordering,
        SortOrder::Desc => ordering.reverse(),
    }
    .then_with(|| left.id.cmp(&right.id))
}

async fn refresh_nodes(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    let info = state.app.refresh_nodes().await?;
    Ok((StatusCode::OK, Json(info)))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConnectionRequest {
    node_id: NodeId,
}

async fn connect(
    State(state): State<ApiState>,
    payload: Result<Json<ConnectionRequest>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Json(request) = payload
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalidJson", "请求 JSON 无效"))?;
    Ok(Json(state.app.connect(request.node_id).await?))
}

async fn disconnect(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(state.app.disconnect().await?))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AcceptedOperation {
    operation_id: OperationId,
}

async fn start_test(
    State(state): State<ApiState>,
    Path(node_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let node_id = NodeId::from_str(&node_id)
        .map_err(|message| ApiError::new(StatusCode::BAD_REQUEST, "invalidNodeId", message))?;
    let operation_id = state.app.enqueue_test(node_id).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(AcceptedOperation { operation_id }),
    ))
}

async fn test_status(
    State(state): State<ApiState>,
    Path(operation_id): Path<String>,
) -> Result<Json<TestState>, ApiError> {
    let operation_id = OperationId::from_str(&operation_id).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalidOperationId",
            "测试操作 ID 无效",
        )
    })?;
    Ok(Json(state.app.test_state(operation_id).await?))
}

async fn status(State(state): State<ApiState>) -> Json<crate::service::StatusSnapshot> {
    Json(state.app.status().await)
}

async fn events(
    State(state): State<ApiState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(state.app.events()).filter_map(|result| match result {
        Ok(event) => serde_json::to_string(&event)
            .ok()
            .map(|json| Ok(Event::default().event("update").data(json))),
        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(_)) => {
            Some(Ok(Event::default().event("resync").data("{}")))
        }
    });
    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn ready(State(state): State<ApiState>) -> StatusCode {
    if state.app.is_ready().await {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn missing_webui() -> impl IntoResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "WebUI 尚未构建；请运行 npm --prefix web run build",
    )
}

async fn api_not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "notFound", "API 路径不存在")
}

async fn api_method_not_allowed() -> ApiError {
    ApiError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "methodNotAllowed",
        "API 方法不受支持",
    )
}

const fn default_page() -> usize {
    1
}

const fn default_page_size() -> usize {
    50
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt as _;
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;
    use tower::ServiceExt as _;

    use crate::{
        config::{AppConfig, Credentials},
        domain::SecretString,
        service::AppState,
        storage::Store,
    };

    use super::*;

    async fn test_router() -> (Router, CancellationToken, tempfile::TempDir) {
        let directory = tempdir().expect("temporary directory");
        let config = AppConfig::test_config(directory.path().to_path_buf());
        let store = Store::open("sqlite::memory:")
            .await
            .expect("in-memory store");
        let shutdown = CancellationToken::new();
        let state = AppState::new(config, store, shutdown.clone());
        (router(state), shutdown, directory)
    }

    #[tokio::test]
    async fn health_and_empty_nodes_are_available_without_lan_auth() {
        let (router, shutdown, _directory) = test_router().await;
        let health = router
            .clone()
            .oneshot(
                Request::get("/healthz")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(health.status(), StatusCode::OK);

        let nodes = router
            .oneshot(
                Request::get("/api/v1/nodes?page=1&pageSize=50")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(nodes.status(), StatusCode::OK);
        let body = nodes.into_body().collect().await.expect("body").to_bytes();
        let value: serde_json::Value = serde_json::from_slice(&body).expect("JSON body");
        assert_eq!(value["total"], 0);
        shutdown.cancel();
    }

    #[tokio::test]
    async fn invalid_connection_body_uses_json_error_envelope() {
        let (router, shutdown, _directory) = test_router().await;
        let response = router
            .oneshot(
                Request::put("/api/v1/connection")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"nodeId":"bad"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let value: serde_json::Value = serde_json::from_slice(&body).expect("JSON body");
        assert_eq!(value["error"]["code"], "invalidJson");
        shutdown.cancel();
    }

    #[tokio::test]
    async fn lan_session_requires_cookie_and_csrf_for_mutations() {
        let directory = tempdir().expect("temporary directory");
        let mut config = AppConfig::test_config(directory.path().to_path_buf());
        config.lan_mode = true;
        config.web_credentials = Some(Credentials {
            username: "operator".to_owned(),
            password: SecretString::new("secret"),
        });
        let store = Store::open("sqlite::memory:")
            .await
            .expect("in-memory store");
        let shutdown = CancellationToken::new();
        let router = router(AppState::new(config, store, shutdown.clone()));

        let unauthorized = router
            .clone()
            .oneshot(
                Request::get("/api/v1/nodes")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let login = router
            .clone()
            .oneshot(
                Request::post("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"username":"operator","password":"secret"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(login.status(), StatusCode::OK);
        let cookie = login
            .headers()
            .get("set-cookie")
            .expect("session cookie")
            .to_str()
            .expect("ASCII cookie")
            .split(';')
            .next()
            .expect("cookie value")
            .to_owned();
        let body = login.into_body().collect().await.expect("body").to_bytes();
        let value: serde_json::Value = serde_json::from_slice(&body).expect("JSON body");
        let csrf = value["csrfToken"].as_str().expect("CSRF token");

        let missing_csrf = router
            .clone()
            .oneshot(
                Request::delete("/api/v1/connection")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);

        let disconnected = router
            .oneshot(
                Request::delete("/api/v1/connection")
                    .header("cookie", cookie)
                    .header("x-csrf-token", csrf)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(disconnected.status(), StatusCode::OK);
        shutdown.cancel();
    }
}
