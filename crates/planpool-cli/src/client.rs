use anyhow::{Result, anyhow, bail};
use planpool_types::{ErrorResponse, PlanCreated};
use ureq::{Agent, Body};

pub struct Client {
    agent: Agent,
    base_url: String,
    token: Option<String>,
}

impl Client {
    pub fn new(base_url: String, token: Option<String>) -> Self {
        // Handle non-2xx ourselves so the server's JSON error body can be
        // relayed instead of ureq's generic status error.
        let agent = Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .new_agent();
        Client {
            agent,
            base_url,
            token,
        }
    }

    fn auth(&self) -> Result<String> {
        self.token
            .as_ref()
            .map(|token| format!("Bearer {token}"))
            .ok_or_else(|| anyhow!("PLANPOOL_TOKEN is not set"))
    }

    pub fn push(&self, html: &[u8], ttl: Option<u64>) -> Result<PlanCreated> {
        let mut url = format!("{}/plans", self.base_url);
        if let Some(ttl) = ttl {
            url.push_str(&format!("?ttl={ttl}"));
        }
        let mut response = self
            .agent
            .post(&url)
            .header("Authorization", self.auth()?)
            .header("Content-Type", "text/html; charset=utf-8")
            .send(html)?;
        if response.status() == 201 {
            Ok(response.body_mut().read_json()?)
        } else {
            Err(api_error(response))
        }
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let response = self
            .agent
            .delete(format!("{}/plans/{id}", self.base_url))
            .header("Authorization", self.auth()?)
            .call()?;
        if response.status() == 204 {
            Ok(())
        } else {
            Err(api_error(response))
        }
    }

    pub fn health(&self) -> Result<()> {
        let response = self
            .agent
            .get(format!("{}/healthz", self.base_url))
            .call()?;
        if response.status().is_success() {
            Ok(())
        } else {
            bail!("healthz returned {}", response.status())
        }
    }

    /// Verifies the token without side effects: an empty-body POST returns
    /// 400 (empty body) when the token is accepted, 401 when it isn't.
    pub fn check_token(&self) -> Result<()> {
        let response = self
            .agent
            .post(format!("{}/plans", self.base_url))
            .header("Authorization", self.auth()?)
            .send_empty()?;
        match response.status().as_u16() {
            400 => Ok(()),
            401 => bail!("token rejected by server"),
            other => bail!("unexpected status {other} from token probe"),
        }
    }
}

fn api_error(mut response: ureq::http::Response<Body>) -> anyhow::Error {
    let status = response.status();
    match response.body_mut().read_json::<ErrorResponse>() {
        Ok(body) => anyhow!("{} ({status})", body.error),
        Err(_) => anyhow!("server returned {status}"),
    }
}
