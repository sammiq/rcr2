use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use std::collections::HashMap;
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
                let mut criteria = HashMap::new();
                let mut fuzzy_criteria = HashMap::new();
                //inserting the name, crc, md5, and sha1 values into the criteria HashMap if they are not None
                if let Some(name) = name {
                    //always fuzzy search by name
                    fuzzy_criteria.insert("name", name.as_str());
                }
                if let Some(crc) = crc {
                    criteria.insert("crc", crc.as_str());
                }
                if let Some(md5) = md5 {
                    criteria.insert("md5", md5.as_str());
                }
                if let Some(sha1) = sha1 {
                    criteria.insert("sha1", sha1.as_str());
                }

                if criteria.is_empty() && fuzzy_criteria.is_empty() {
                    return Err(anyhow!("Please provide at least one search criterion (name, crc, md5, or sha1)"));
                }

                let results = db.search_roms(&criteria, &fuzzy_criteria).context("Failed to search database")?;
                if !results.is_empty() {
                    println!("Found {} matching game(s)", results.len());
                    for (game, roms) in results {
                        print_game_with_roms(&game, &roms);
                    }
                } else {
                    let args = criteria.iter().chain(fuzzy_criteria.iter()).map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join(", ");
                    println!("No ROMs found matching criteria: {}", args);
                }
            }
        },
    }

    Ok(())
}
