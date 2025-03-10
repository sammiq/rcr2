use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use clap::Subcommand;

use crate::{database, models, xml_parser};

#[derive(Subcommand)]
pub enum DbCommands {
    /// Initialize the database
    Initialize {
        /// Path to the XML file to import
        #[arg(short, long)]
        input: PathBuf,
    },
    /// Import data into the database
    Import {
        /// Path to the XML file to import
        #[arg(short, long)]
        input: PathBuf,
    },
    /// Search the database
    Search {
        #[command(subcommand)]
        search_type: SearchType,
    },
}

#[derive(Subcommand)]
pub enum SearchType {
    /// Search by game name
    Game {
        /// Game name to search for (fuzzy search)
        #[arg(short, long)]
        name: String,
    },
    /// Search for ROMs by various criteria
    Rom {
        /// ROM name to search for (fuzzy search)
        #[arg(short, long)]
        name: Option<String>,

        /// ROM CRC to search for (exact match)
        #[arg(short, long)]
        crc: Option<String>,

        /// ROM MD5 to search for (exact match)
        #[arg(short, long)]
        md5: Option<String>,

        /// ROM SHA1 to search for (exact match)
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

pub fn handle_command(db_path: &Path, debug: bool, command: &DbCommands) -> Result<()> {
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
            let mut db = database::check_for_database(db_path, debug)?;
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
                SearchType::Rom { name, crc, md5, sha1 } => {
                    search_roms(&db, name, crc, md5, sha1)?;
                }
            }
        }
    }
    Ok(())
}

fn search_roms(
    db: &database::Database,
    name: &Option<String>,
    crc: &Option<String>,
    md5: &Option<String>,
    sha1: &Option<String>,
) -> Result<()> {
    let mut criteria = HashMap::new();
    let mut fuzzy_criteria = HashMap::new();
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
    let results = db
        .search_roms(&criteria, &fuzzy_criteria)
        .context("Failed to search database")?;
    if results.is_empty() {
        let args = criteria
            .iter()
            .chain(&fuzzy_criteria)
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join(", ");
        println!("No ROMs found matching criteria: {}", args);
    } else {
        println!("Found {} matching game(s)", results.len());
        for (game, roms) in results {
            print_game_with_roms(&game, &roms);
        }
    }
    Ok(())
}
