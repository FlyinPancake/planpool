mod client;

use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use clap::{CommandFactory, Parser, Subcommand};

use crate::client::Client;

/// Environment configuration; flags override where noted.
#[derive(confroid::Config)]
struct EnvConfig {
    /// Base URL of the planpool server
    #[confroid(name = "PLANPOOL_URL")]
    url: Option<String>,
    /// Bearer token for push/delete
    #[confroid(name = "PLANPOOL_TOKEN")]
    token: Option<String>,
}

#[derive(Parser)]
#[command(
    name = "pp",
    version,
    about = "planpool CLI — push HTML plans, get shareable URLs",
    after_help = "Configuration: PLANPOOL_URL (server base URL), PLANPOOL_TOKEN (bearer token).\n\
                  The result goes to stdout, everything else to stderr."
)]
struct Cli {
    /// Server base URL (overrides PLANPOOL_URL)
    #[arg(long, global = true)]
    url: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Upload an HTML plan and print its URL
    Push {
        /// Plan file; reads stdin when omitted or "-"
        file: Option<PathBuf>,
        /// Plan lifetime: plain seconds ("3600") or humantime ("1h", "7days")
        #[arg(long, value_parser = parse_ttl)]
        ttl: Option<u64>,
        /// Print the full JSON response instead of just the URL
        #[arg(long)]
        json: bool,
    },
    /// Delete a plan early
    Delete {
        /// Plan ID or full plan URL
        plan: String,
    },
    /// Open a plan in the browser
    Open {
        /// Plan ID or full plan URL
        plan: String,
    },
    /// Check that the server is reachable and the token (if set) is valid
    Health,
    /// Print shell completions
    Completions { shell: clap_complete::Shell },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let env: EnvConfig = confroid::from_env().map_err(|e| anyhow!("{e}"))?;

    match cli.command {
        Command::Push { file, ttl, json } => {
            let html = read_input(file.as_deref())?;
            let created = make_client(&cli.url, env)?.push(&html, ttl)?;
            let lifetime =
                Duration::from_secs(created.expires_at.saturating_sub(created.created_at));
            eprintln!(
                "pushed {} ({} bytes), expires in {}",
                created.id,
                html.len(),
                humantime::format_duration(lifetime)
            );
            if json {
                println!("{}", serde_json::to_string_pretty(&created)?);
            } else {
                println!("{}", created.url);
            }
        }
        Command::Delete { plan } => {
            let id = extract_id(&plan)?;
            make_client(&cli.url, env)?.delete(&id)?;
            eprintln!("deleted {id}");
        }
        Command::Open { plan } => {
            let target = if plan.starts_with("http://") || plan.starts_with("https://") {
                extract_id(&plan)?; // validate it actually points at a plan
                plan
            } else {
                let id = extract_id(&plan)?;
                format!("{}/plans/{id}", base_url(&cli.url, &env)?)
            };
            eprintln!("opening {target}");
            open::that_detached(&target).with_context(|| format!("cannot open {target}"))?;
        }
        Command::Health => {
            let has_token = env.token.is_some();
            let client = make_client(&cli.url, env)?;
            client.health()?;
            eprintln!("server ok");
            if has_token {
                client.check_token()?;
                eprintln!("token ok");
            } else {
                eprintln!("token not set (PLANPOOL_TOKEN) — push/delete unavailable");
            }
        }
        Command::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "pp", &mut std::io::stdout());
        }
    }
    Ok(())
}

fn base_url(flag: &Option<String>, env: &EnvConfig) -> Result<String> {
    flag.clone()
        .or_else(|| env.url.clone())
        .map(|url| url.trim_end_matches('/').to_string())
        .ok_or_else(|| anyhow!("no server URL: set PLANPOOL_URL or pass --url"))
}

fn make_client(flag: &Option<String>, env: EnvConfig) -> Result<Client> {
    Ok(Client::new(base_url(flag, &env)?, env.token))
}

fn read_input(file: Option<&std::path::Path>) -> Result<Vec<u8>> {
    let html = match file {
        Some(path) if path.as_os_str() != "-" => {
            std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?
        }
        _ => {
            let mut stdin = std::io::stdin();
            if stdin.is_terminal() {
                bail!("no input: pass a FILE or pipe HTML on stdin");
            }
            let mut buf = Vec::new();
            stdin.read_to_end(&mut buf).context("cannot read stdin")?;
            buf
        }
    };
    if html.is_empty() {
        bail!("input is empty");
    }
    Ok(html)
}

/// Accepts plain seconds ("3600") or a humantime string ("1h", "7days").
fn parse_ttl(raw: &str) -> Result<u64, String> {
    if let Ok(secs) = raw.parse::<u64>() {
        return Ok(secs);
    }
    humantime::parse_duration(raw)
        .map(|d| d.as_secs())
        .map_err(|_| {
            format!("`{raw}` is not seconds or a humantime duration (try \"1h\", \"7days\")")
        })
}

/// Extracts a plan ID from a bare ID or any URL ending in /plans/{id}.
fn extract_id(input: &str) -> Result<String> {
    let tail = input.trim_end_matches('/');
    let candidate = tail.rsplit('/').next().unwrap_or(tail);
    let candidate = candidate.split(['?', '#']).next().unwrap_or(candidate);
    if is_plan_id(candidate) {
        Ok(candidate.to_string())
    } else {
        bail!("`{input}` is not a plan ID or plan URL");
    }
}

fn is_plan_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttl_accepts_seconds_and_humantime() {
        assert_eq!(parse_ttl("3600"), Ok(3600));
        assert_eq!(parse_ttl("1h"), Ok(3600));
        assert_eq!(parse_ttl("7days"), Ok(604_800));
        assert_eq!(parse_ttl("1h 30m"), Ok(5400));
        assert!(parse_ttl("soon").is_err());
        assert!(parse_ttl("-5").is_err());
    }

    #[test]
    fn extracts_id_from_bare_and_url_forms() {
        let id = "879255f0c80239b707ef77159a2d7980";
        assert_eq!(extract_id(id).unwrap(), id);
        assert_eq!(
            extract_id(&format!("https://plans.example.com/plans/{id}")).unwrap(),
            id
        );
        assert_eq!(
            extract_id(&format!("http://127.0.0.1:8642/plans/{id}/")).unwrap(),
            id
        );
        assert_eq!(
            extract_id(&format!("https://plans.example.com/plans/{id}?x=1#top")).unwrap(),
            id
        );
        assert!(extract_id("not-a-plan").is_err());
        assert!(extract_id("https://plans.example.com/plans/").is_err());
        assert!(extract_id("https://plans.example.com/docs").is_err());
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
