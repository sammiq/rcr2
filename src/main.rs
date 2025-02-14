use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use crc32fast::Hasher;
use md5::Md5;
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use strum::{Display, IntoStaticStr};

mod database;
mod models;
mod xml_parser;

macro_rules! debug_log {
    ($debug:expr, $($arg:tt)*) => {
        if $debug {
            eprintln!("{}", format!("Debug: {}", format!($($arg)*)));
        }
    };
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to the SQLite database
    #[arg(short, long, default_value = ".rcr.db")]
    database: PathBuf,

    /// Enable debug output
    #[arg(long)]
    debug: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, IntoStaticStr, Display)]
enum HashMethod {
    /// CRC32 hash
    Crc,
    /// MD5 hash
    Md5,
    /// SHA1 hash
    Sha1,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, IntoStaticStr, Display)]
enum DisplayMethod {
    /// Display exact matches only
    Exact,
    /// Display partial matches only
    Partial,
    /// Display misses only
    Miss,
    /// Display all partial and miss matches
    NotExact,
}

#[derive(Subcommand)]
enum Commands {
    /// Perform a database operation
    Database {
        
        #[command(subcommand)]
        db_command: DbCommands,
    },
    /// Perform a file operation
    File {
        #[command(subcommand)]
        file_command: FileCommands,
    }
}

#[derive(Subcommand)]
enum FileCommands {
    /// Scan all files in the directory for ROMs
    FullScan {
        /// Hash method to use
        #[arg(short, long, value_enum, default_value = "sha1")]
        method: HashMethod,

        /// Display method for files
        #[arg(long, value_enum, default_value = "not-exact")]
        file_display: DisplayMethod,

        /// Stop after first partial match for each file
        #[arg(short, long)]
        first_match: bool,

        /// Directory to scan (defaults to current directory)
        #[arg(short, long, default_value = ".")]
        directory: PathBuf,
    },
}

#[derive(Subcommand)]
enum DbCommands {
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
    }
}

#[derive(Subcommand)]
enum SearchType {
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

fn calculate_hash(data: &[u8], hash_type: HashMethod) -> Result<String> {
    match hash_type {
        HashMethod::Crc => {
            let mut hasher = Hasher::new();
            hasher.update(data);
            let checksum = hasher.finalize();
            Ok(format!("{:08x}", checksum))
        }
        HashMethod::Md5 => {
            let mut hasher = Md5::new();
            hasher.update(data);
            let result = hasher.finalize();
            Ok(format!("{:x}", result))
        }
        HashMethod::Sha1 => {
            let mut hasher = Sha1::new();
            hasher.update(data);
            let result = hasher.finalize();
            Ok(format!("{:x}", result))
        }
    }
}

#[derive(Default)]
struct GameStatus {
    total_roms: usize,
    exact_matches: HashSet<String>,                    // ROM names
    partial_matches: HashMap<String, HashSet<String>>, // ROM names
}

fn scan_directory(
    db: &database::Database,
    hash_type: HashMethod,
    directory: &PathBuf,
    first_match: bool,
    file_display: DisplayMethod,
    debug: bool,
) -> Result<()> {
    // Verify directory exists and is a directory
    if !directory.exists() {
        return Err(anyhow!("Directory does not exist: {}", directory.display()));
    }
    if !directory.is_dir() {
        return Err(anyhow!("Not a directory: {}", directory.display()));
    }

    println!("Scanning directory: {}", directory.display());
    debug_log!(debug, "Using hash type: {}", hash_type);

    let mut game_status: BTreeMap<String, GameStatus> = BTreeMap::new();

    // Read directory contents and sort by path
    let mut paths: Vec<_> = fs::read_dir(directory)?
        .filter_map(|r| r.ok())
        .collect();
    paths.sort_by_key(|dir| dir.path());

    for entry in paths {
        let path = entry.path();

        // Skip directories and non-files
        if !path.is_file() {
            continue;
        }

        let filename = path
            .file_name()
            .and_then(|n| n.to_str());

        // Skip files with strange names
        if filename.is_none() {
            continue;
        }

        let path = path.to_str();
        // Skip files with strange paths
        if path.is_none() {
            continue;
        }
        
        let path = path.unwrap();
        let filename = filename.unwrap();
            
        // Skip hidden files
        if filename.starts_with('.') {
            continue;
        }

        debug_log!(debug, "\nDebug: Processing file: {}", filename);

        // Read file contents
        let mut file = fs::File::open(&path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        // Calculate hash
        let hash = calculate_hash(&buffer, hash_type)?;

        debug_log!(debug, "Calculated {} hash: {}", hash_type, hash);

        // Search database for matches
        let mut criteria = HashMap::new();
        criteria.insert(hash_type.into(), hash.as_str());

        let results = db.search_roms(&criteria, &HashMap::new())?;

        if results.is_empty() {
            debug_log!(debug, "No matches found in database");
            if file_display == DisplayMethod::Miss || file_display == DisplayMethod::NotExact {
                println!("[MISS] {}", filename);
            }
            db.store_file(
                &path,
                &hash,
                &hash_type.to_string(),
                "miss",
                None,
                None,
            )?;
        } else {
            debug_log!(debug, "Found {} matching entries in database", results.len());

            let mut exact_match = None;
            let mut partial_matches = Vec::new();

            for (game, roms) in results {
                let game_entry = game_status.entry(game.name.clone()).or_insert_with(|| {
                    let num_roms = db
                        .search_by_game_name(&game.name, false)
                        .expect("Game could not be found in database")
                        .first()
                        .expect("Game could not be found in database")
                        .roms
                        .len();
                    GameStatus {
                        total_roms: num_roms,
                        exact_matches: HashSet::new(),
                        partial_matches: HashMap::new(),
                    }
                });
                for rom in roms {
                    if debug {
                        debug_log!(debug, "Comparing with database entry:");
                        debug_log!(debug, "  Game: {}", game.name);
                        debug_log!(debug, "  ROM: {}", rom.name);
                        debug_log!(debug, "  Size: {}", rom.size);
                        match hash_type {
                            HashMethod::Crc => {
                                if let Some(h) = &rom.crc {
                                    debug_log!(debug, "  CRC: {}", h)
                                }
                            }
                            HashMethod::Md5 => {
                                if let Some(h) = &rom.md5 {
                                    debug_log!(debug, "  MD5: {}", h)
                                }
                            }
                            HashMethod::Sha1 => {
                                if let Some(h) = &rom.sha1 {
                                    debug_log!(debug, "  SHA1: {}", h)
                                }
                            }
                        }
                    }

                    if rom.name == filename {
                        debug_log!(debug, "Found exact match");
                        exact_match = Some(game.name.clone());
                        game_entry.exact_matches.insert(filename.to_string());
                        db.store_file(
                            &path,
                            &hash,
                            &hash_type.to_string(),
                            "exact",
                            Some(&game.name),
                            Some(&filename),
                        )?;
                        break;
                    } else {
                        partial_matches.push((game.name.clone(), rom.name.clone()));
                        let partials = game_entry.partial_matches.entry(rom.name.clone()).or_default();
                        partials.insert(filename.to_string());
                        db.store_file(
                            &path,
                            &hash,
                            &hash_type.to_string(),
                            "partial",
                            Some(&game.name),
                            Some(&filename),
                        )?;
                    }
                }
            }

            match exact_match {
                Some(game_name) => {
                    if file_display == DisplayMethod::Exact {
                        println!("[OK  ] {} ({})", filename, game_name);
                    }
                }
                None => {
                    // If we only have partial matches, print all of them
                    for (game_name, rom_name) in partial_matches {
                        if file_display == DisplayMethod::Partial || file_display == DisplayMethod::NotExact {
                            println!("[WARN] {} (Expected: {}, Game: {})", filename, rom_name, game_name);
                        }
                        if first_match {
                            break;
                        }
                    }
                }
            }
        }
    }

    // Print summary
    println!("\nGame Summary:");
    for (game_name, status) in game_status.iter() {
        let exact_count = status.exact_matches.len();
        let partial_count = status.partial_matches.len();

        if exact_count > 0 || partial_count == status.total_roms {
            if exact_count == status.total_roms {
                println!("[OK  ] {}", game_name);
            } else {
                println!(
                    "[WARN] {} ({} exact matches, {} partial matches, {} total required)",
                    game_name, exact_count, partial_count, status.total_roms
                );
                for (expected, partial_match) in status.partial_matches.iter() {
                    for filename in partial_match {
                        println!("\t[WARN] {} (Expected: {})", filename, expected);
                    }
                }
            }
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut db = database::Database::new(&cli.database).context("Failed to connect to database")?;

    match &cli.command {
        Commands::Database { db_command } => match db_command{
            DbCommands::Initialize { input } => {
                db.initialize().context("Failed to initialize database")?;
    
                let data = xml_parser::parse_file(input).context("Failed to parse XML file")?;
    
                db.merge_data(data).context("Failed to merge data into database")?;
    
                println!("Initialize completed successfully");
            }
            DbCommands::Import { input } => {
                let data = xml_parser::parse_file(input).context("Failed to parse XML file")?;
    
                db.merge_data(data).context("Failed to merge data into database")?;
    
                println!("Import completed successfully");
            }
            DbCommands::Search { search_type } => match search_type {
                SearchType::Game { name } => {
                    let results = db.search_by_game_name(name, true).context("Failed to search database")?;
                    if !results.is_empty() {
                        println!("Found {} matching game(s)", results.len());
                        for game in results {
                            print_game_with_roms(&game, &game.roms);
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
    
                    let results = db
                        .search_roms(&criteria, &fuzzy_criteria)
                        .context("Failed to search database")?;
                    if !results.is_empty() {
                        println!("Found {} matching game(s)", results.len());
                        for (game, roms) in results {
                            print_game_with_roms(&game, &roms);
                        }
                    } else {
                        let args = criteria
                            .iter()
                            .chain(fuzzy_criteria.iter())
                            .map(|(k, v)| format!("{k}: {v}"))
                            .collect::<Vec<_>>()
                            .join(", ");
                        println!("No ROMs found matching criteria: {}", args);
                    }
                }
            },
        }
        Commands::File { file_command } => match file_command {
            FileCommands::FullScan {
                method,
                file_display,
                first_match,
                directory,
            } => {
                scan_directory(&db, *method, directory, *first_match, *file_display, cli.debug).context("Failed to scan directory")?;
            }
        }
    }

    Ok(())
}
