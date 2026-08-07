use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use planpool_types::{ErrorResponse, PlanCreated};
use serde::Deserialize;
use subtle::ConstantTimeEq;
use utoipa::{IntoParams, Modify, OpenApi};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use utoipa_scalar::{Scalar, Servable};

use crate::AppState;
use crate::config::Config;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "planpool",
        description = "A small pool of expiring static HTML plans. Agents POST a plan \
                       and get back a shareable URL; the plan is served until its TTL \
                       runs out, then disappears.\n\nUploads and deletes require the \
                       bearer token. Viewing needs only the unguessable plan URL.",
    ),
    modifiers(&BearerAuth),
    tags(
        (name = "plans", description = "Upload, view, and delete plans"),
        (name = "meta", description = "Service endpoints"),
    )
)]
struct ApiDoc;

struct BearerAuth;

impl Modify for BearerAuth {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{Http, HttpAuthScheme, SecurityScheme};
        openapi
            .components
            .get_or_insert_default()
            .add_security_scheme(
                "bearer_token",
                SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
            );
    }
}

pub fn router(state: AppState) -> Router {
    // routes!() registers each handler and its #[utoipa::path] spec entry in
    // one step, so the served routes and the OpenAPI document can't drift.
    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(healthz))
        .routes(routes!(upload))
        .routes(routes!(view, remove))
        .split_for_parts();

    let spec = api.clone();
    router
        .route("/", get(index))
        .route(
            "/api-docs/openapi.json",
            get(move || {
                let spec = spec.clone();
                async move { Json(spec) }
            }),
        )
        .merge(Scalar::with_url("/docs", api))
        .layer(DefaultBodyLimit::max(
            usize::try_from(state.config.max_body_bytes).unwrap_or(usize::MAX),
        ))
        .with_state(state)
}

async fn index() -> &'static str {
    concat!(
        "planpool ",
        env!("CARGO_PKG_VERSION"),
        "\n\n",
        "POST   /plans[?ttl=<seconds>]  upload an HTML plan (Bearer auth), returns its URL\n",
        "GET    /plans/{id}             view a plan\n",
        "DELETE /plans/{id}             remove a plan early (Bearer auth)\n",
        "\n",
        "API docs: /docs (OpenAPI spec: /api-docs/openapi.json)\n",
    )
}

/// Liveness check.
#[utoipa::path(get, path = "/healthz", tag = "meta", responses(
    (status = 200, description = "Service is up", body = String, example = json!("ok"))
))]
async fn healthz() -> &'static str {
    "ok"
}

#[derive(Deserialize, IntoParams)]
struct UploadQuery {
    /// How long the plan should live, in seconds. Defaults to the server's
    /// default TTL and is clamped to its maximum. (Human-friendly forms like
    /// "1h" are a CLI concern; the wire format is canonical seconds.)
    ttl: Option<u64>,
}

/// Upload an HTML plan.
///
/// The raw request body is stored verbatim and served back as HTML. Returns
/// the plan's unguessable URL, which is the only way to view it.
#[utoipa::path(
    post,
    path = "/plans",
    tag = "plans",
    params(UploadQuery),
    request_body(content = String, content_type = "text/html", description = "The plan document"),
    responses(
        (status = 201, description = "Plan stored", body = PlanCreated),
        (status = 400, description = "Empty request body", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 413, description = "Body exceeds the upload size limit"),
    ),
    security(("bearer_token" = []))
)]
async fn upload(
    State(state): State<AppState>,
    Query(query): Query<UploadQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(denied) = require_auth(&headers, &state.config) {
        return denied;
    }
    if body.is_empty() {
        return error(StatusCode::BAD_REQUEST, "request body is empty");
    }
    let ttl = query
        .ttl
        .unwrap_or(state.config.default_ttl.as_secs())
        .clamp(1, state.config.max_ttl.as_secs());

    match state.store.put(&body, ttl).await {
        Ok(meta) => {
            tracing::info!(id = %meta.id, size = meta.size, ttl, "plan uploaded");
            (
                StatusCode::CREATED,
                Json(PlanCreated {
                    url: plan_url(&state.config, &headers, &meta.id),
                    id: meta.id,
                    created_at: meta.created_at,
                    expires_at: meta.expires_at,
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("failed to store plan: {e}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "failed to store plan")
        }
    }
}

/// View a plan.
#[utoipa::path(
    get,
    path = "/plans/{id}",
    tag = "plans",
    params(("id" = String, Path, description = "32-char hex plan ID")),
    responses(
        (status = 200, description = "The plan document", content_type = "text/html"),
        (status = 404, description = "Plan does not exist or has expired", body = ErrorResponse),
    )
)]
async fn view(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let found = match state.store.get(&id).await {
        Ok(found) => found,
        Err(e) => {
            tracing::error!(id, "failed to read plan: {e}");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "failed to read plan");
        }
    };
    let Some((path, _meta)) = found else {
        return error(StatusCode::NOT_FOUND, "plan not found or expired");
    };
    match tokio::fs::read(&path).await {
        Ok(html) => (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            html,
        )
            .into_response(),
        Err(_) => error(StatusCode::NOT_FOUND, "plan not found or expired"),
    }
}

/// Delete a plan before it expires.
#[utoipa::path(
    delete,
    path = "/plans/{id}",
    tag = "plans",
    params(("id" = String, Path, description = "32-char hex plan ID")),
    responses(
        (status = 204, description = "Plan deleted"),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 404, description = "Plan does not exist or has expired", body = ErrorResponse),
    ),
    security(("bearer_token" = []))
)]
async fn remove(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(denied) = require_auth(&headers, &state.config) {
        return denied;
    }
    match state.store.delete(&id).await {
        Ok(true) => {
            tracing::info!(id, "plan deleted");
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => error(StatusCode::NOT_FOUND, "plan not found or expired"),
        Err(e) => {
            tracing::error!(id, "failed to delete plan: {e}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "failed to delete plan")
        }
    }
}

/// Returns the 401 response to send, or None if the request is authorized.
fn require_auth(headers: &HeaderMap, config: &Config) -> Option<Response> {
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if provided.as_bytes().ct_eq(config.token.as_bytes()).into() {
        None
    } else {
        Some(error(
            StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        ))
    }
}

fn plan_url(config: &Config, headers: &HeaderMap, id: &str) -> String {
    if let Some(base) = &config.public_url {
        format!("{base}/plans/{id}")
    } else if let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) {
        format!("http://{host}/plans/{id}")
    } else {
        format!("/plans/{id}")
    }
}

fn error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: message.to_string(),
        }),
    )
        .into_response()
}
