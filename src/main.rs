use anyhow::Result;
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand, ValueEnum};
use file_commands::StorageType;
use strum::{Display, IntoStaticStr};

mod cache;
mod cache_commands;
mod database;
mod db_commands;
mod file_commands;
mod models;
mod xml_parser;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Enable debug output
    #[arg(long)]
    debug: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, ValueEnum, IntoStaticStr, Display)]
enum StorageMode {
    /// Use the in-memory cache
    Cache,
    /// Use the SQLite database
    Database,
}

#[derive(Subcommand)]
enum Commands {
    /// Perform a cache operation
    Cache {
        #[command(subcommand)]
        cache_command: cache_commands::CacheCommands,

        /// Path to the cache
        #[arg(short, long, default_value = ".rcr.rbc")]
        cache: Utf8PathBuf,
    },
    /// Perform a database operation
    Database {
        #[command(subcommand)]
        db_command: db_commands::DbCommands,

        /// Path to the database
        #[arg(short, long, default_value = ".rcr.db")]
        database: Utf8PathBuf,
    },
    /// Perform a file operation
    File {
        #[command(subcommand)]
        file_command: file_commands::FileCommands,

        /// Path to the database, if database in use
        #[arg(short, long, default_value = ".rcr.db")]
        database: Utf8PathBuf,

        /// Path to the cache, if cache in use
        #[arg(short, long, default_value = ".rcr.rbc")]
        cache: Utf8PathBuf,

        /// Storage mode to use
        #[arg(short, long, default_value = "cache")]
        storage: StorageMode,

        /// List of file extensions to exclude, comma separated
        #[arg(short, long, value_delimiter = ',', default_value = "m3u,dat")]
        exclude_extensions: Vec<String>,
    },
}

fn main() -> Result<()> {
    let mut cli = Cli::parse();

    match &mut cli.command {
        Commands::Cache { cache_command, cache } => cache_commands::handle_command(cache, cli.debug, cache_command),
        Commands::Database { db_command, database } => db_commands::handle_command(database, cli.debug, db_command),
        Commands::File {
            file_command,
            database,
            cache,
            storage,
            exclude_extensions,
        } => {
            let mut storage_type: StorageType = match storage {
                StorageMode::Cache => StorageType::Cache(cache::check_for_cache(cache, cli.debug)?),
                StorageMode::Database => StorageType::Database(database::check_for_database(database, cli.debug)?),
            };

            file_commands::handle_command(&mut storage_type, cli.debug, file_command, exclude_extensions)
        }
    }
}
