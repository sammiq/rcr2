use anyhow::{Context, Ok, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod database;
mod db_commands;
mod file_commands;
mod models;
mod xml_parser;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to the database
    #[arg(short, long, default_value = ".rcr.db")]
    database: PathBuf,

    /// Enable debug output
    #[arg(long)]
    debug: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Perform a database operation
    Database {
        #[command(subcommand)]
        db_command: db_commands::DbCommands,
    },
    /// Perform a file operation
    File {
        #[command(subcommand)]
        file_command: file_commands::FileCommands,

        /// List of file extensions to exclude, comma separated
        #[arg(short, long, value_delimiter = ',', default_value = "m3u,dat")]
        exclude_extensions: Vec<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut db = database::Database::new(&cli.database).context("Failed to connect to database")?;

    match &cli.command {
        Commands::Database { db_command } => db_commands::handle_command(&mut db, db_command)?,
        Commands::File {
            file_command,
            exclude_extensions,
        } => file_commands::handle_command(&mut db, cli.debug, file_command, exclude_extensions)?,
    }

    Ok(())
}
