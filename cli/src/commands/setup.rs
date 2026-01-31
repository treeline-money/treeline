//! Setup command - configure integrations for syncing financial data

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;

use super::{get_context, get_logger, get_treeline_dir, log_event};
use treeline_core::LogEvent;

/// Environment variable for Lunchflow API key
const LUNCHFLOW_API_KEY_ENV: &str = "LUNCHFLOW_API_KEY";

#[derive(Subcommand)]
pub enum SetupCommands {
    /// Set up SimpleFIN integration
    #[command(name = "simplefin")]
    SimpleFIN {
        /// Setup token from SimpleFIN Bridge (get one at https://beta-bridge.simplefin.org/)
        token: String,
    },
    /// Set up Lunchflow integration
    #[command(name = "lunchflow")]
    Lunchflow {
        /// API key from Lunchflow dashboard (or set LUNCHFLOW_API_KEY env var)
        api_key: Option<String>,
        /// Custom API base URL (for testing)
        #[arg(long)]
        base_url: Option<String>,
    },
    /// Show configured integrations
    Status,
    /// Remove an integration
    Remove {
        /// Integration name to remove (e.g., simplefin, lunchflow)
        name: String,
    },
}

pub fn run(command: Option<SetupCommands>) -> Result<()> {
    let logger = get_logger();

    match command {
        Some(SetupCommands::SimpleFIN { token }) => {
            log_event(
                &logger,
                LogEvent::new("setup_started").with_integration("simplefin"),
            );

            println!("Setting up SimpleFIN integration...");

            let ctx = get_context()?;
            match ctx.sync_service.setup_simplefin(&token) {
                Ok(()) => {
                    log_event(
                        &logger,
                        LogEvent::new("setup_completed").with_integration("simplefin"),
                    );
                    println!("{}", "SimpleFIN configured successfully!".green());
                    println!();
                    println!("Run '{}' to sync your accounts.", "tl sync".cyan());
                    Ok(())
                }
                Err(e) => {
                    log_event(
                        &logger,
                        LogEvent::new("setup_failed")
                            .with_integration("simplefin")
                            .with_error(&e.to_string()),
                    );
                    Err(e)
                }
            }
        }
        Some(SetupCommands::Lunchflow { api_key, base_url }) => {
            log_event(
                &logger,
                LogEvent::new("setup_started").with_integration("lunchflow"),
            );

            // Try to get API key from argument, then environment variable
            let api_key = api_key
                .or_else(|| std::env::var(LUNCHFLOW_API_KEY_ENV).ok())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Lunchflow API key required. Provide as argument or set {} environment variable.\n\n\
                        To get your API key:\n\
                        1. Create an account at https://www.lunchflow.app/?atp=treeline\n\
                        2. Connect your bank accounts\n\
                        3. Create an API destination in the dashboard\n\
                        4. Copy your API key from the destination settings",
                        LUNCHFLOW_API_KEY_ENV
                    )
                })?;

            // Also check for base URL from environment if not provided
            let base_url = base_url.or_else(|| std::env::var("LUNCHFLOW_BASE_URL").ok());

            println!("Setting up Lunchflow integration...");

            let treeline_dir = get_treeline_dir();
            std::fs::create_dir_all(&treeline_dir)?;

            let ctx = get_context()?;
            match ctx.sync_service.setup_lunchflow(&api_key, base_url.as_deref()) {
                Ok(()) => {
                    log_event(
                        &logger,
                        LogEvent::new("setup_completed").with_integration("lunchflow"),
                    );
                    println!("{}", "Lunchflow configured successfully!".green());
                    println!();
                    println!("Run '{}' to sync your accounts.", "tl sync".cyan());
                    Ok(())
                }
                Err(e) => {
                    log_event(
                        &logger,
                        LogEvent::new("setup_failed")
                            .with_integration("lunchflow")
                            .with_error(&e.to_string()),
                    );
                    Err(e)
                }
            }
        }
        Some(SetupCommands::Status) => {
            let ctx = get_context()?;
            let integrations = ctx.sync_service.list_integrations()?;

            if integrations.is_empty() {
                println!("{}", "No integrations configured.".yellow());
                println!();
                show_available_integrations();
            } else {
                println!("{}", "Configured integrations:".green());
                for integration in integrations {
                    println!("  - {}", integration.name);
                }
            }
            Ok(())
        }
        Some(SetupCommands::Remove { name }) => {
            log_event(
                &logger,
                LogEvent::new("setup_remove").with_integration(&name),
            );

            let ctx = get_context()?;
            match ctx.sync_service.remove_integration(&name) {
                Ok(()) => {
                    println!("{} integration removed.", name.green());
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        None => {
            // Show help when no subcommand provided
            show_available_integrations();
            Ok(())
        }
    }
}

fn show_available_integrations() {
    println!("Available integrations:");
    println!();
    println!("  {} - Global bank connections (20,000+ institutions)", "lunchflow".cyan());
    println!("    tl setup lunchflow <api_key>");
    println!("    Or set {} environment variable", "LUNCHFLOW_API_KEY".yellow());
    println!();
    println!("    To get your API key:");
    println!("    1. Create an account at https://www.lunchflow.app/?atp=treeline");
    println!("    2. Connect your bank accounts");
    println!("    3. Create an API destination in the dashboard");
    println!("    4. Copy your API key from the destination settings");
    println!();
    println!("  {} - US/Canada bank connections", "simplefin".cyan());
    println!("    tl setup simplefin <token>");
    println!("    Get a setup token: https://beta-bridge.simplefin.org/");
    println!();
    println!("Use '{}' to see configured integrations.", "tl setup status".cyan());
}
