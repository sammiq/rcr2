use anyhow::{anyhow, Context, Ok, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use crc32fast::Hasher;
use md5::Md5;
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
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

#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, ValueEnum, IntoStaticStr, Display)]
enum DisplayMethod {
    /// Display exact matches only
    Exact,
    /// Display partial matches only
    Partial,
    /// Display misses only
    Miss,
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

        /// List of file extensions to exclude, comma separated
        #[arg(short, long, value_delimiter = ',', default_value = "m3u,dat")]
        exclude_extensions: Vec<String>,
    },
}

#[derive(Args)]
struct ScanArgs {
    /// Hash method to use
    #[arg(short, long, value_enum, default_value = "sha1")]
    method: HashMethod,

    /// Display method for files
    #[arg(long, value_enum, value_delimiter = ',', default_value = "exact,partial,miss")]
    file_display: Vec<DisplayMethod>,

    /// Stop after first partial match for each file
    #[arg(short, long)]
    first_match: bool,

    /// Directory to scan (defaults to current directory)
    #[arg(short, long, default_value = ".")]
    directory: PathBuf,

    ///Rename files if unambiguous match is found
    #[arg(short, long)]
    rename: bool,
}

#[derive(Subcommand)]
enum FileCommands {
    /// Scan all files in the directory and store the results in the database
    Scan(ScanArgs),
    //Update the database with the files in the directory, skipping files that are already in the database
    Update(ScanArgs),
    /// Check all files in the directory against the database
    Check {
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
    },
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

fn should_skip_file(path: &Path, exclude_extensions: &Vec<String>) -> bool {
    // Skip directories and non-files
    if !path.is_file() {
        return true;
    }

    let extension = path.extension().and_then(|n| n.to_str());
    // Skip files with strange extensions
    if extension.is_none() {
        return true;
    }

    let filename = path.file_name().and_then(|n| n.to_str());
    // Skip files with strange names
    if filename.is_none() {
        return true;
    }

    let path = path.to_str();
    // Skip files with strange paths
    if path.is_none() {
        return true;
    }

    let filename = filename.unwrap();
    let extension = extension.unwrap();

    // Skip hidden files
    if filename.starts_with('.') {
        return true;
    }

    exclude_extensions.contains(&extension.to_string())
}

fn read_and_hash_file(path: &str, method: HashMethod, debug: bool) -> Result<String> {
    // Read file contents
    let mut file = fs::File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    // Calculate hash
    let hash = calculate_hash(&buffer, method)?;
    debug_log!(debug, "Calculated {} hash: {}", method, hash);

    Ok(hash)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut db = database::Database::new(&cli.database).context("Failed to connect to database")?;

    match &cli.command {
        Commands::Database { db_command } => match db_command {
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
        },
        Commands::File {
            file_command,
            exclude_extensions,
        } => match file_command {
            FileCommands::Scan(args) => {
                scan_directory(&db, args, cli.debug, exclude_extensions).context("Failed to scan directory")?;
            }
            FileCommands::Update(args) => {
                update_directory(&db, args, cli.debug, exclude_extensions).context("Failed to update directory")?;
            }
            FileCommands::Check { directory } => {
                check_directory(&db, directory, cli.debug, exclude_extensions).context("Failed to check directory")?;
            }
        },
    }

    Ok(())
}

fn scan_directory(db: &database::Database, args: &ScanArgs, debug: bool, exclude_extensions: &Vec<String>) -> Result<()> {
    // Verify directory exists and is a directory
    if !args.directory.exists() {
        return Err(anyhow!("Directory does not exist: {}", args.directory.display()));
    }
    if !args.directory.is_dir() {
        return Err(anyhow!("Not a directory: {}", args.directory.display()));
    }

    let base_path = args.directory.to_str().ok_or_else(|| anyhow!("Invalid base path"))?;
    let hash_method: &str = args.method.into();

    println!("Scanning directory: {}", base_path);
    debug_log!(debug, "Using hash type: {}", hash_method);

    let mut game_status: BTreeMap<String, GameStatus> = BTreeMap::new();

    // Read directory contents and sort by path
    let mut paths: Vec<_> = fs::read_dir(&args.directory)?.filter_map(|r| r.ok()).collect();
    paths.sort_by_key(|dir| dir.path());

    //before we start scanning the directory, we need to clear the database of any files that have the same base path
    db.clear_files_by_base_path(base_path)?;

    for entry in paths {
        let path = entry.path();

        if should_skip_file(&path, exclude_extensions) {
            continue;
        }
        let filename = path.file_name().unwrap().to_str().unwrap();
        let path = path.to_str().unwrap();

        debug_log!(debug, "\nDebug: Processing file: {}", filename);

        let hash = read_and_hash_file(path, args.method, debug)?;

        // Search database for matches
        let mut criteria = HashMap::new();
        criteria.insert(hash_method, hash.as_str());

        let results = db.search_roms(&criteria, &HashMap::new())?;

        let mut scanned_file = models::ScannedFile {
            base_path: base_path.to_string(),
            path: path.to_string(),
            hash: hash.clone(),
            hash_type: hash_method.to_string(),
            match_type: "miss".to_string(),
            game_name: None,
            rom_name: None,
        };

        if results.is_empty() {
            debug_log!(debug, "No matches found in database");
            if args.file_display.contains(&DisplayMethod::Miss) {
                println!("[MISS] {} {}", hash, filename);
            }
            db.store_file(&scanned_file)?;
        } else {
            debug_log!(debug, "Found {} matching entries in database", results.len());

            let (exact_match, partial_matches) = check_matches(&db, args, debug, filename, &results, &mut game_status)?;

            handle_rom_matches(&db, args, debug, filename, &mut scanned_file, exact_match, partial_matches)?;
        }
    }

    // Print summary
    println!("\nGame Summary:");
    for (game_name, status) in game_status.iter() {
        let exact_count = status.exact_matches.len();
        let partial_count = status.partial_matches.len();

        if exact_count > 0 || (exact_count + partial_count) == status.total_roms {
            if exact_count == status.total_roms {
                println!("[OK  ] {}", game_name);
            } else {
                println!("[WARN] {} ({} exact matches, {} partial matches)", game_name, exact_count, partial_count);
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

fn check_matches(
    db: &database::Database,
    args: &ScanArgs,
    debug: bool,
    filename: &str,
    results: &Vec<(models::Game, Vec<models::Rom>)>,
    game_status: &mut BTreeMap<String, GameStatus>,
) -> Result<(Option<String>, Vec<(String, String)>)> {
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
                match args.method {
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
                break;
            } else {
                partial_matches.push((game.name.clone(), rom.name.clone()));
                let partials = game_entry.partial_matches.entry(rom.name.clone()).or_default();
                partials.insert(filename.to_string());
            }
        }
    }
    Ok((exact_match, partial_matches))
}

fn handle_rom_matches(
    db: &database::Database,
    args: &ScanArgs,
    debug: bool,
    filename: &str,
    scanned_file: &mut models::ScannedFile,
    exact_match: Option<String>,
    partial_matches: Vec<(String, String)>,
) -> Result<()> {
    match exact_match {
        Some(game_name) => {
            if args.file_display.contains(&DisplayMethod::Exact) {
                println!("[OK  ] {} {} (Game: {})", scanned_file.hash, filename, game_name);
            }

            scanned_file.match_type = "exact".to_string();
            scanned_file.game_name = Some(game_name);
            scanned_file.rom_name = Some(filename.to_string());
            db.store_file(&scanned_file)?;
        }
        None => {
            // Rename file if we have a single partial match
            if args.rename && partial_matches.len() == 1 {
                let (game_name, rom_name) = partial_matches.first().unwrap();
                let mut new_pathname = args.directory.clone();
                new_pathname.push(rom_name);
                debug_log!(debug, "Renaming file from: {} to: {}", scanned_file.path, new_pathname.display());
                fs::rename(&scanned_file.path, new_pathname)?;
                if args.file_display.contains(&DisplayMethod::Exact) {
                    println!("[OK  ] {} {} (Game: {})", scanned_file.hash, rom_name, game_name);
                }

                scanned_file.match_type = "exact".to_string();
                scanned_file.game_name = Some(game_name.to_owned());
                scanned_file.rom_name = Some(rom_name.to_owned());
                db.store_file(&scanned_file)?;
            } else {
                let mut display_match = true;
                // If we only have partial matches, print all of them
                for (game_name, rom_name) in partial_matches {
                    if display_match && args.file_display.contains(&DisplayMethod::Partial) {
                        println!("[WARN] {} {} (Expected: {}, Game: {})", scanned_file.hash, filename, rom_name, game_name);
                        display_match = !args.first_match;
                    }

                    scanned_file.match_type = "partial".to_string();
                    scanned_file.game_name = Some(game_name);
                    scanned_file.rom_name = Some(rom_name);
                    db.store_file(&scanned_file)?;
                }
            }
        }
    }
    Ok(())
}

fn update_directory(db: &database::Database, args: &ScanArgs, debug: bool, exclude_extensions: &Vec<String>) -> Result<()> {
    // Verify directory exists and is a directory
    if !args.directory.exists() {
        return Err(anyhow!("Directory does not exist: {}", args.directory.display()));
    }
    if !args.directory.is_dir() {
        return Err(anyhow!("Not a directory: {}", args.directory.display()));
    }

    let base_path = args.directory.to_str().ok_or_else(|| anyhow!("Invalid base path"))?;
    let hash_method: &str = args.method.into();

    println!("Updating directory: {}", base_path);
    debug_log!(debug, "Using hash type: {}", hash_method);

    // Get all entries in the database with the same base path
    let files = db.get_files_by_base_path(base_path)?;
    let mut db_files = BTreeMap::new();
    for file in files {
        db_files.insert(file.path.clone(), file);
    }

    // Read directory contents and sort by path
    let mut paths: Vec<_> = fs::read_dir(&args.directory)?.filter_map(|r| r.ok()).collect();
    paths.sort_by_key(|dir| dir.path());
    // for each file in the directory, check if its in the database or not
    // and report it on the console
    for entry in paths {
        let path = entry.path();
        // Skip directories and non-files

        if should_skip_file(&path, exclude_extensions) {
            continue;
        }

        let filename = path.file_name().unwrap().to_str().unwrap();
        let path = path.to_str().unwrap();

        debug_log!(debug, "\nDebug: Processing file: {}", filename);

        if let Some(_scanned_file) = db_files.remove(path) {
            //skip this file if it is already in the database
        } else {
            //doesn't seem to be in the database, so check the hash
            let hash = read_and_hash_file(path, args.method, debug)?;
        }
    }

    Ok(())
}

fn check_directory(db: &database::Database, directory: &PathBuf, debug: bool, exclude_extensions: &Vec<String>) -> Result<()> {
    // Verify directory exists and is a directory
    if !directory.exists() {
        return Err(anyhow!("Directory does not exist: {}", directory.display()));
    }
    if !directory.is_dir() {
        return Err(anyhow!("Not a directory: {}", directory.display()));
    }

    let base_path = directory.to_str().ok_or_else(|| anyhow!("Invalid base path"))?;

    println!("Checking directory: {}", base_path);

    // Get all entries in the database with the same base path
    let files = db.get_files_by_base_path(base_path)?;
    // Create a HashMap of the files in the database
    let mut db_files = BTreeMap::new();
    for file in files {
        db_files.insert(file.path.clone(), file);
    }
    // Read directory contents and sort by path
    let mut paths: Vec<_> = fs::read_dir(directory)?.filter_map(|r| r.ok()).collect();
    paths.sort_by_key(|dir| dir.path());
    // for each file in the directory, check if its in the database or not
    // and report it on the console
    for entry in paths {
        let path = entry.path();
        // Skip directories and non-files

        if should_skip_file(&path, exclude_extensions) {
            continue;
        }

        let path = path.to_str().unwrap();

        if let Some(scanned_file) = db_files.remove(path) {
            let hash = read_and_hash_file(path, HashMethod::from_str(&scanned_file.hash_type, true).unwrap(), debug)?;
            if hash != scanned_file.hash {
                println!("[ERR ] {} {} (Expected: {})", hash, scanned_file.path, scanned_file.hash);
            } else {
                match scanned_file.match_type.as_str() {
                    "exact" => {
                        println!("[OK  ] {} {} (Game: {})", scanned_file.hash, scanned_file.path, scanned_file.game_name.unwrap());
                    }
                    "partial" => {
                        println!(
                            "[WARN] {} {} (Expected: {}, Game: {})",
                            scanned_file.hash,
                            scanned_file.path,
                            scanned_file.rom_name.unwrap(),
                            scanned_file.game_name.unwrap()
                        );
                    }
                    _ => {
                        println!("[MISS] {} {}", scanned_file.hash, scanned_file.path);
                    }
                }
            }
        } else {
            println!("[NEW ] {}", path);
        }
    }

    // Print entries in the database that were not found in the directory
    if !db_files.is_empty() {
        for (_, file) in db_files {
            println!("[GONE] {} {}", file.hash, file.path);
        }
    }

    Ok(())
}
