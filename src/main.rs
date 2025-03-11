use anyhow::Result;
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};

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
    database: Utf8PathBuf,

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
    let mut cli = Cli::parse();

    match &mut cli.command {
        Commands::Database { db_command } => db_commands::handle_command(&cli.database, cli.debug, db_command),
        Commands::File {
            file_command,
            exclude_extensions,
        } => {
            let mut db = database::check_for_database(&cli.database, cli.debug)?;
            file_commands::handle_command(&mut db, cli.debug, file_command, exclude_extensions)
        }
    }
}
