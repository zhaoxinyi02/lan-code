use std::{
    env,
    io::{self, Write},
    sync::Arc,
};

use anyhow::{Context, Result};
use lan_core::{AgentCore, LanConfig, SqliteStore};
use lan_protocol::{ApprovalMode, SessionId};

const BANNER: &str = r#"
 _                    ____          _
| |    __ _ _ __    / ___|___   __| | ___
| |   / _` | '_ \  | |   / _ \ / _` |/ _ \
| |__| (_| | | | | | |__| (_) | (_| |  __/
|_____\__,_|_| |_|  \____\___/ \__,_|\___|
                 local-first coding agent
"#;

fn parse_mode(value: &str) -> Option<ApprovalMode> {
    match value {
        "read-only" | "readonly" => Some(ApprovalMode::ReadOnly),
        "ask" => Some(ApprovalMode::Ask),
        "workspace" => Some(ApprovalMode::Workspace),
        "full-access" | "fullaccess" => Some(ApprovalMode::FullAccess),
        _ => None,
    }
}

fn build_core(config: &LanConfig) -> Result<AgentCore> {
    let provider = config
        .provider()?
        .context("configure [provider] in lan.toml or set DEEPSEEK_API_KEY")?;
    match config.database() {
        Some(path) => AgentCore::with_provider_and_store(
            Arc::new(provider),
            SqliteStore::open(path).context("open configured database")?,
        ),
        None => Ok(AgentCore::with_provider(Arc::new(provider))),
    }
}

async fn run_prompt(
    core: &AgentCore,
    session_id: SessionId,
    prompt: String,
    mode: ApprovalMode,
) -> Result<()> {
    let result = core.start_turn(session_id, prompt, mode).await?;
    println!("\n{}\n", result.text);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = LanConfig::load()?;
    let core = build_core(&config)?;
    let cwd = env::current_dir()?.display().to_string();
    let mut mode = config.approval_mode()?;
    let args = env::args().skip(1).collect::<Vec<_>>();

    if !args.is_empty() {
        let session = core.create_session(cwd, Some("CLI session".into())).await;
        return run_prompt(&core, session.id, args.join(" "), mode).await;
    }

    println!("{BANNER}");
    println!("Workspace: {cwd}");
    println!("Mode: {mode:?}");
    println!("Commands: /new, /mode <read-only|ask|workspace|full-access>, /clear, /exit\n");

    let mut session = core
        .create_session(cwd.clone(), Some("Interactive CLI".into()))
        .await;
    loop {
        print!("lan> ");
        io::stdout().flush()?;
        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            break;
        }
        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        if input == "/exit" || input == "/quit" {
            break;
        }
        if input == "/new" {
            session = core
                .create_session(cwd.clone(), Some("Interactive CLI".into()))
                .await;
            println!("Started a new session.\n");
            continue;
        }
        if input == "/clear" {
            print!("\x1B[2J\x1B[1;1H");
            io::stdout().flush()?;
            continue;
        }
        if let Some(value) = input.strip_prefix("/mode ") {
            match parse_mode(value.trim()) {
                Some(next) => {
                    mode = next;
                    println!("Mode: {mode:?}\n");
                }
                None => println!("Unknown mode.\n"),
            }
            continue;
        }
        if let Err(error) = run_prompt(&core, session.id, input.to_string(), mode).await {
            eprintln!("Error: {error:#}\n");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use lan_protocol::ApprovalMode;

    use super::parse_mode;

    #[test]
    fn parses_cli_modes() {
        assert_eq!(parse_mode("workspace"), Some(ApprovalMode::Workspace));
        assert_eq!(parse_mode("full-access"), Some(ApprovalMode::FullAccess));
        assert_eq!(parse_mode("unknown"), None);
    }
}
