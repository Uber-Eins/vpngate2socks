//! Cookie session and CSRF protection used only when LAN mode is enabled.

use std::{collections::HashMap, sync::Arc};

use axum::{
    Json,
    extract::{Request, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse as _, Response},
};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;
use tokio::sync::RwLock;
use uuid::Uuid;
use zeroize::Zeroize as _;

use crate::config::Credentials;

use super::{ApiError, ApiState};

const SESSION_COOKIE: &str = "vpngate2socks_session";
const SESSION_HOURS: i64 = 8;

#[derive(Clone)]
pub(super) struct AuthManager {
    credentials: Option<Credentials>,
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    secure_cookie: bool,
}

#[derive(Clone)]
struct Session {
    csrf_token: String,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SessionResponse {
    authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    csrf_token: Option<String>,
}

impl AuthManager {
    pub(super) fn new(credentials: Option<Credentials>, secure_cookie: bool) -> Self {
        Self {
            credentials,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            secure_cookie,
        }
    }

    fn is_required(&self) -> bool {
        self.credentials.is_some()
    }

    async fn session(&self, headers: &HeaderMap) -> Option<(String, Session)> {
        if !self.is_required() {
            return None;
        }
        let token = cookie(headers, SESSION_COOKIE)?;
        let session = self.sessions.read().await.get(token).cloned()?;
        if session.expires_at <= Utc::now() {
            self.sessions.write().await.remove(token);
            return None;
        }
        Some((token.to_owned(), session))
    }
}

pub(super) async fn login(
    State(state): State<ApiState>,
    payload: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(mut request) = payload
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalidJson", "请求 JSON 无效"))?;
    let Some(expected) = state.auth.credentials.as_ref() else {
        request.password.zeroize();
        return Ok(Json(SessionResponse {
            authenticated: true,
            csrf_token: None,
        })
        .into_response());
    };
    let supplied_user = Sha256::digest(request.username.as_bytes());
    let expected_user = Sha256::digest(expected.username.as_bytes());
    let supplied_password = Sha256::digest(request.password.as_bytes());
    let expected_password = Sha256::digest(expected.password.expose().as_bytes());
    request.password.zeroize();
    let valid = supplied_user.ct_eq(&expected_user) & supplied_password.ct_eq(&expected_password);
    if !bool::from(valid) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalidCredentials",
            "用户名或密码错误",
        ));
    }

    let token = Uuid::new_v4().simple().to_string();
    let csrf_token = Uuid::new_v4().simple().to_string();
    let expires_at = Utc::now() + TimeDelta::hours(SESSION_HOURS);
    state.auth.sessions.write().await.insert(
        token.clone(),
        Session {
            csrf_token: csrf_token.clone(),
            expires_at,
        },
    );
    let mut response = Json(SessionResponse {
        authenticated: true,
        csrf_token: Some(csrf_token),
    })
    .into_response();
    let secure = if state.auth.secure_cookie {
        "; Secure"
    } else {
        ""
    };
    let cookie = format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{}",
        SESSION_HOURS * 60 * 60,
        secure
    );
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).map_err(|_| ApiError::internal())?,
    );
    Ok(response)
}

pub(super) async fn session(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Json<SessionResponse> {
    if !state.auth.is_required() {
        return Json(SessionResponse {
            authenticated: true,
            csrf_token: None,
        });
    }
    let session = state
        .auth
        .session(&headers)
        .await
        .map(|(_, session)| session);
    Json(SessionResponse {
        authenticated: session.is_some(),
        csrf_token: session.map(|session| session.csrf_token),
    })
}

pub(super) async fn logout(State(state): State<ApiState>, headers: HeaderMap) -> Response {
    if let Some(token) = cookie(&headers, SESSION_COOKIE) {
        state.auth.sessions.write().await.remove(token);
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    let cookie = if state.auth.secure_cookie {
        HeaderValue::from_static(
            "vpngate2socks_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0; Secure",
        )
    } else {
        HeaderValue::from_static(
            "vpngate2socks_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0",
        )
    };
    response.headers_mut().insert(header::SET_COOKIE, cookie);
    response
}

pub(super) async fn require_auth(
    State(state): State<ApiState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    if !state.auth.is_required() {
        return Ok(next.run(request).await);
    }
    let (_, session) = state.auth.session(request.headers()).await.ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "authenticationRequired",
            "需要登录",
        )
    })?;
    if !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        let supplied = request
            .headers()
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok());
        let Some(supplied) = supplied else {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "csrfRequired",
                "缺少 CSRF 令牌",
            ));
        };
        if !constant_time_equal(supplied.as_bytes(), session.csrf_token.as_bytes()) {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "csrfInvalid",
                "CSRF 令牌无效",
            ));
        }
    }
    Ok(next.run(request).await)
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|part| {
            part.strip_prefix(name)
                .and_then(|value| value.strip_prefix('='))
        })
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let left = Sha256::digest(left);
    let right = Sha256::digest(right);
    bool::from(left.ct_eq(&right))
}
