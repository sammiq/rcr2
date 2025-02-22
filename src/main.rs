use anyhow::{anyhow, Result};
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

    match &cli.command {
        Commands::Database { db_command } => {db_commands::handle_command(&cli.database, cli.debug, db_command)},
        Commands::File {
            file_command,
            exclude_extensions,
        } => {
            if let Some(mut db) = database::check_for_database(&cli.database, cli.debug) {
                file_commands::handle_command(&mut db, cli.debug, file_command, exclude_extensions)
            } else {
                Err(anyhow!("Database file {} does not exist, please initialize the database first", cli.database.display()))
            }
        },
    }
}
