use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use clap::{Subcommand, ValueEnum};

use crate::{database, models, xml_parser};

#[derive(Subcommand)]
pub enum DbCommands {
    /// Initialize the database
    Initialize {
        /// Path to the XML file to import
        input: Utf8PathBuf,
    },
    /// Import data into the database
    Import {
        /// Path to the XML file to import
        input: Utf8PathBuf,
    },
    /// Search the database
    Search {
        #[command(subcommand)]
        search_type: SearchType,
    },
}

#[derive(ValueEnum, Clone)]
pub enum SearchCriteria {
    /// ROM name to search for (fuzzy search)
    Name,
    /// ROM CRC to search for (exact match)
    Crc,
    /// ROM MD5 to search for (exact match)
    Md5,
    /// ROM SHA1 to search for (exact match)
    Sha1,
}

#[derive(Subcommand)]
pub enum SearchType {
    /// Search by game name
    Game {
        /// Game name to search for (fuzzy search)
        name: String,
    },
    /// Search for ROMs by various criteria
    Rom {
        /// Search criteria to use
        #[arg(short, long, default_value = "Name")]
        mode: SearchCriteria,

        /// Text to search for
        text: String,
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

pub fn handle_command(db_path: &Utf8Path, debug: bool, command: &DbCommands) -> Result<()> {
    match command {
        DbCommands::Initialize { input } => {
            let mut db = database::Database::new(db_path).context("Failed to connect to database")?;
            db.initialize().context("Failed to initialize database")?;
            let data = xml_parser::parse_file(input).context("Failed to parse XML file")?;
            db.merge_data(data).context("Failed to merge data into database")?;
            println!("Initialize completed successfully");
        }
        DbCommands::Import { input } => {
            let mut db = database::check_for_database(db_path, debug)?;
            let data = xml_parser::parse_file(input).context("Failed to parse XML file")?;
            db.merge_data(data).context("Failed to merge data into database")?;
            println!("Import completed successfully");
        }
        DbCommands::Search { search_type } => {
            let db = database::check_for_database(db_path, debug)?;
            match search_type {
                SearchType::Game { name } => {
                    let results = db.search_by_game_name(name, true).context("Failed to search database")?;
                    if results.is_empty() {
                        println!("No games found matching name: {}", name);
                    } else {
                        println!("Found {} matching game(s)", results.len());
                        for game in results {
                            print_game_with_roms(&game, &game.roms);
                        }
                    }
                }
                SearchType::Rom { mode, text } => {
                    search_roms(&db, mode, text)?;
                }
            }
        }
    }
    Ok(())
}

fn search_roms(db: &database::Database, mode: &SearchCriteria, search_term: &str) -> Result<()> {
    let criteria = match mode {
        SearchCriteria::Name => ("name", search_term, true),
        SearchCriteria::Crc => ("crc", search_term, false),
        SearchCriteria::Md5 => ("md5", search_term, false),
        SearchCriteria::Sha1 => ("sha1", search_term, false),
    };

    let results = db.search_roms(criteria).context("Failed to search database")?;
    if results.is_empty() {
        println!("No ROMs found matching criteria: {:?}", criteria);
    } else {
        println!("Found {} matching game(s)", results.len());
        for (game, roms) in results {
            print_game_with_roms(&game, &roms);
        }
    }
    Ok(())
}
