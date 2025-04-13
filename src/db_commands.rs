use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use clap::Subcommand;

use crate::{database, models, xml_parser};

#[derive(Subcommand)]
pub enum DbCommands {
    /// Initialize the database
    Initialize {
        /// Path to the XML file to import
        input: Utf8PathBuf,

        /// List of remappings for file extensions, comma separated
        /// e.g. "3ds=cci,bin=nes"
        #[arg(short, long, value_delimiter = ',', value_parser = parse_key_val::<String, String>)]
        remap_extensions: Vec<(String, String)>,
    },
    /// Import data into the database
    Import {
        /// Path to the XML file to import
        input: Utf8PathBuf,

        /// List of remappings for file extensions, comma separated
        /// e.g. "3ds=cci,bin=nes"
        #[arg(short, long, value_delimiter = ',', value_parser = parse_key_val::<String, String>)]
        remap_extensions: Vec<(String, String)>,
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
        name: String,
    },
    /// Search for ROMs by various criteria
    Rom {
        /// ROM name to search for (fuzzy search)
        name: Option<String>,

        /// CRC to search for (exact match)
        #[arg(short, long)]
        crc: Option<String>,

        /// MD5 to search for (exact match)
        #[arg(short, long)]
        md5: Option<String>,

        /// SHA1 to search for (exact match)
        #[arg(short, long)]
        sha1: Option<String>,
    },
}

fn parse_key_val<T, U>(s: &str) -> Result<(T, U), Box<dyn std::error::Error + Send + Sync + 'static>>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
    U: std::str::FromStr,
    U::Err: std::error::Error + Send + Sync + 'static,
{
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=value: no `=` found in `{s}`"))?;
    Ok((s[..pos].parse()?, s[pos + 1..].parse()?))
}

fn print_game_with_roms(game: &models::Game, roms: &[models::Rom]) {
    println!("\nGame:");
    println!("Name: {}", game.name);
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
        DbCommands::Initialize { input, remap_extensions } => {
            let mut db = database::Database::new(db_path).context("Failed to connect to database")?;
            db.initialize().context("Failed to initialize database")?;
            let mut data = xml_parser::parse_file(input).context("Failed to parse XML file")?;
            if !remap_extensions.is_empty() {
                let remap: HashMap<String, String> = remap_extensions.iter().cloned().collect();
                remap_datafile(&mut data, &remap).context("Failed to remap datafile")?;
            }
            db.merge_data(data).context("Failed to merge data into database")?;
            println!("Initialize completed successfully");
        }
        DbCommands::Import { input, remap_extensions } => {
            let mut db = database::check_for_database(db_path, debug)?;
            let mut data = xml_parser::parse_file(input).context("Failed to parse XML file")?;
            if !remap_extensions.is_empty() {
                let remap: HashMap<String, String> = remap_extensions.iter().cloned().collect();
                remap_datafile(&mut data, &remap).context("Failed to remap datafile")?;
            }
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
                SearchType::Rom { name, crc, md5, sha1 } => {
                    search_roms(&db, name, crc, md5, sha1)?;
                }
            }
        }
    }
    Ok(())
}

fn remap_datafile(data: &mut models::DataFile, remap_extensions: &HashMap<String, String>) -> Result<()> {
    for game in &mut data.games {
        for rom in &mut game.roms {
            let mut iter = rom.name.rsplitn(2, '.');
            let after = iter.next();
            let before = iter.next();
            if before == Some("") {
                continue;
            } else {
                if let Some(extension) = before.and(after) {
                    if let Some(new_extension) = remap_extensions.get(extension) {
                        rom.name = format!("{}.{}", before.unwrap(), new_extension);
                    }
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

    if criteria.is_empty() {
        Err(anyhow!("No criteria given on command line, please supply at least one search term"))
    } else {
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
}
