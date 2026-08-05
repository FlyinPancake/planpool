//! Request/response types shared between the planpool server and its clients.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Returned by `POST /plans` when a plan is stored.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlanCreated {
    /// 32-char hex plan ID; unguessable, acts as the view capability.
    #[schema(example = "879255f0c80239b707ef77159a2d7980")]
    pub id: String,
    /// Absolute URL where the plan can be viewed.
    #[schema(example = "https://plans.example.com/plans/879255f0c80239b707ef77159a2d7980")]
    pub url: String,
    /// Unix timestamp (seconds) when the plan was stored.
    pub created_at: u64,
    /// Unix timestamp (seconds) after which the plan is gone.
    pub expires_at: u64,
}

/// Error body returned by every non-2xx JSON response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ErrorResponse {
    #[schema(example = "plan not found or expired")]
    pub error: String,
}
