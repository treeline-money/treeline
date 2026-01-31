//! Demo command - manage demo mode

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;

use super::get_treeline_dir;
use treeline_core::services::DemoService;

#[derive(Subcommand)]
pub enum DemoCommands {
    /// Enable demo mode
    #[command(name = "on")]
    On,
    /// Disable demo mode
    #[command(name = "off")]
    Off,
    /// Show demo mode status
    Status,
}

pub fn run(command: Option<DemoCommands>) -> Result<()> {
    let treeline_dir = get_treeline_dir();
    std::fs::create_dir_all(&treeline_dir)?;
    let demo_service = DemoService::new(&treeline_dir);

    match command {
        Some(DemoCommands::On) => {
            demo_service.enable()?;
            println!("{}", "Demo mode enabled".green());
            println!("Demo data has been populated. Run 'tl status' to see your demo accounts.");
            Ok(())
        }
        Some(DemoCommands::Off) => {
            demo_service.disable(false)?; // Don't delete demo data by default
            println!("{}", "Demo mode disabled".yellow());
            Ok(())
        }
        Some(DemoCommands::Status) | None => {
            if demo_service.is_enabled()? {
                println!("Demo mode is {}", "ON".green());
            } else {
                println!("Demo mode is {}", "OFF".yellow());
            }
            Ok(())
        }
    }
}
