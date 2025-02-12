use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod database;
mod models;
mod xml_parser;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to the SQLite database
    #[arg(short, long, default_value = ".rcr.db")]
    database: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Initialize {
        /// Path to the XML file
        #[arg(short, long)]
        input: PathBuf,
    },
    /// Import XML data into the database
    Import {
        /// Path to the XML file
        #[arg(short, long)]
        input: PathBuf,
    },
    /// Search operations
    Search {
        #[command(subcommand)]
        search_type: SearchType,
    },
}

#[derive(Subcommand)]
enum SearchType {
    /// Search by game name
    Game {
        /// Game name to search for
        #[arg(short, long)]
        name: String,
    },
    /// Search for ROMs by various criteria
    Rom {
        /// ROM name to search for
        #[arg(short, long)]
        name: Option<String>,

        /// ROM CRC to search for
        #[arg(short, long)]
        crc: Option<String>,

        /// ROM MD5 to search for
        #[arg(short, long)]
        md5: Option<String>,

        /// ROM SHA1 to search for
        #[arg(short, long)]
        sha1: Option<String>,
    },
}

fn print_game_with_roms(game: &models::Game, roms: &[models::Rom]) {
    println!("\nGame:");
    println!("Name: {}", game.name);
    println!("Category: {}", game.category);
    //    println!("Description: {}", game.description);
    println!("ROMs:");
    for rom in roms {
        println!("\n\tName: {}", rom.name);
        println!("\tSize: {}", rom.size);
        if let Some(crc) = &rom.crc {
            println!("\tCRC: {}", crc);
        }
        if let Some(md5) = &rom.md5 {
            println!("\tMD5: {}", md5);
        }
        if let Some(sha1) = &rom.sha1 {
            println!("\tSHA1: {}", sha1);
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut db = database::Database::new(&cli.database).context("Failed to connect to database")?;

    match &cli.command {
        Commands::Initialize { input } => {
            db.initialize().context("Failed to initialize database")?;

            let data = xml_parser::parse_file(input).context("Failed to parse XML file")?;

            db.merge_data(data).context("Failed to merge data into database")?;

            println!("Initialize completed successfully");
        }
        Commands::Import { input } => {
            let data = xml_parser::parse_file(input).context("Failed to parse XML file")?;

            db.merge_data(data).context("Failed to merge data into database")?;

            println!("Import completed successfully");
        }
        Commands::Search { search_type } => match search_type {
            SearchType::Game { name } => {
                let results = db.search_by_game_name(name).context("Failed to search database")?;
                if !results.is_empty() {
                    println!("Found {} matching game(s)", results.len());
                    for (game, roms) in results {
                        print_game_with_roms(&game, &roms);
                    }
                } else {
                    println!("No games found matching name: {}", name);
                }
            }
            SearchType::Rom { name, crc, md5, sha1 } => {
                if name.is_none() && crc.is_none() && md5.is_none() && sha1.is_none() {
                    return Err(anyhow!("Please provide at least one search criterion (name, crc, md5, or sha1)"));
                }

                let results = db
                    .search_roms(name.as_deref(), crc.as_deref(), md5.as_deref(), sha1.as_deref())
                    .context("Failed to search database")?;
                if !results.is_empty() {
                    println!("Found {} matching game(s)", results.len());
                    for (game, roms) in results {
                        print_game_with_roms(&game, &roms);
                    }
                } else {
                    let criteria = vec![
                        name.as_ref().map(|n| format!("name: {}", n)),
                        crc.as_ref().map(|c| format!("crc: {}", c)),
                        md5.as_ref().map(|m| format!("md5: {}", m)),
                        sha1.as_ref().map(|s| format!("sha1: {}", s)),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(", ");

                    println!("No ROMs found matching criteria: {}", criteria);
                }
            }
        },
    }

    Ok(())
}
