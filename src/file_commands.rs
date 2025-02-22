use anyhow::{anyhow, Context, Ok, Result};
use clap::{Args, Subcommand, ValueEnum};
use crc32fast::Hasher;
use md5::Md5;
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, DirEntry};
use std::io::Read;
use std::path::{Path, PathBuf};
use strum::{Display, IntoStaticStr};

use crate::models::Rom;
use crate::{database, models};

macro_rules! debug_log {
    ($debug:expr, $($arg:tt)*) => {
        if $debug {
            eprintln!("{}", format!("Debug: {}", format!($($arg)*)));
        }
    };
}

#[derive(Subcommand)]
pub enum FileCommands {
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

#[derive(Args)]
pub struct ScanArgs {
    /// Hash method to use
    #[arg(short, long, value_enum, default_value = "sha1")]
    method: HashMethod,

    /// Display method for files
    #[arg(long, value_enum, value_delimiter = ',', default_value = "exact,partial,miss")]
    file_display: Vec<DisplayMethod>,

    /// Stop after first match for each file
    #[arg(short, long, default_value = "true")]
    first_match: bool,

    /// Directory to scan (defaults to current directory)
    #[arg(short, long, default_value = ".")]
    directory: PathBuf,

    ///Rename files if unambiguous match is found
    #[arg(short, long)]
    rename: bool,
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

#[derive(Default)]
struct GameStatus {
    roms: Vec<Rom>,
    exact_matches: HashSet<String>,
    partial_matches: HashMap<String, HashSet<String>>,
}

pub fn handle_command(
    db: &mut database::Database,
    debug: bool,
    command: &FileCommands,
    exclude_extensions: &[String],
) -> Result<()> {
    match command {
        FileCommands::Scan(args) => {
            scan_directory(db, args, debug, exclude_extensions).context("Failed to scan directory")?;
        }
        FileCommands::Update(args) => {
            update_directory(db, args, debug, exclude_extensions).context("Failed to update directory")?;
        }
        FileCommands::Check { directory } => {
            check_directory(db, directory, debug, exclude_extensions).context("Failed to check directory")?;
        }
    }
    Ok(())
}

fn scan_directory(db: &database::Database, args: &ScanArgs, debug: bool, exclude_extensions: &[String]) -> Result<()> {
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

    let mut found_games: BTreeMap<String, GameStatus> = BTreeMap::new();

    // Read directory contents and sort by path
    let mut paths: Vec<_> = fs::read_dir(&args.directory)?.filter_map(Result::ok).collect();
    paths.sort_by_key(DirEntry::path);

    //before we start scanning the directory, we need to clear the database of any files that have the same base path
    db.clear_files_by_base_path(base_path)?;

    for entry in paths {
        let path = entry.path();

        if should_skip_file(&path, exclude_extensions) {
            continue;
        }

        debug_log!(debug, "\nDebug: Processing file: {}", path.display());
        let hash = read_and_hash_file(&path, args.method, debug)?;
        match_roms(db, args, debug, base_path, &path, &hash, &mut found_games)?;
    }

    print_found_games(&found_games);

    Ok(())
}

fn update_directory(db: &database::Database, args: &ScanArgs, debug: bool, exclude_extensions: &[String]) -> Result<()> {
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

    let mut found_games: BTreeMap<String, GameStatus> = BTreeMap::new();
    let mut hash_to_file: BTreeMap<String, HashSet<String>> = BTreeMap::new();

    // Read directory contents and sort by path
    let mut paths: Vec<_> = fs::read_dir(&args.directory)?.filter_map(Result::ok).collect();
    paths.sort_by_key(DirEntry::path);

    for entry in paths {
        let path = entry.path();
        // Skip directories and non-files

        if should_skip_file(&path, exclude_extensions) {
            continue;
        }

        let filename = path
            .file_name()
            .ok_or_else(|| anyhow!("should have a file name"))?
            .to_str()
            .ok_or_else(|| anyhow!("should have a unicode file name"))?;
        let path_str = path.to_str().ok_or_else(|| anyhow!("Invalid path"))?;

        if let Some(scanned_file) = db_files.remove(path_str) {
            //just treat the database as correct, and add it to the game status
            if let Some(game_name) = scanned_file.game_name {
                let game_status = get_game_status(db, &mut found_games, &game_name);
                let rom_name = scanned_file.rom_name.ok_or_else(|| anyhow!("should have a rom name"))?;
                if scanned_file.match_type == "exact" {
                    game_status.exact_matches.insert(rom_name);
                } else {
                    let partials = game_status.partial_matches.entry(rom_name).or_default();
                    partials.insert(filename.to_owned());
                }
            }
        } else {
            debug_log!(debug, "\nDebug: Processing file: {}", filename);

            //doesn't seem to be in the database, so check the hash and add it to the database
            let hash = read_and_hash_file(&path, args.method, debug)?;

            //store the file and the hash in a hash table so that we can find renamed files
            hash_to_file.entry(hash.clone()).or_default().insert(filename.to_owned());

            match_roms(db, args, debug, base_path, &path, &hash, &mut found_games)?;
        }
    }

    debug_log!(debug, "Hash to file: {:?}", hash_to_file);

    //if there are missing file then we should remove them from the database, but we need to check if they were renamed first
    for db_file in db_files.values() {
        debug_log!(debug, "Checking missing file: {} with hash: {}", db_file.path, db_file.hash);
        if let Some(filenames) = hash_to_file.get(&db_file.hash) {
            if filenames.len() == 1 {
                //we have a single file with the same hash, so we can assume it was renamed
                debug_log!(debug, "deleting database entry: {}", db_file.path);
                db.delete_file(&db_file.path)?;
            }

            println!("[MOVE] {} {}", db_file.hash, db_file.path);
        } else {
            println!("[GONE] {} {}", db_file.hash, db_file.path);
        }
    }

    print_found_games(&found_games);

    Ok(())
}

fn check_directory(db: &database::Database, directory: &PathBuf, debug: bool, exclude_extensions: &[String]) -> Result<()> {
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
    let mut paths: Vec<_> = fs::read_dir(directory)?.filter_map(Result::ok).collect();
    paths.sort_by_key(DirEntry::path);
    // for each file in the directory, check if its in the database or not
    // and report it on the console
    for entry in paths {
        let path = entry.path();
        // Skip directories and non-files

        if should_skip_file(&path, exclude_extensions) {
            continue;
        }
        let path_str = path.to_str().ok_or_else(|| anyhow!("Invalid path"))?;

        if let Some(scanned_file) = db_files.remove(path_str) {
            let hash_method = HashMethod::from_str(&scanned_file.hash_type, false).expect("should always be a valid hash method");
            let hash = read_and_hash_file(&path, hash_method, debug)?;
            if hash == scanned_file.hash {
                match scanned_file.match_type.as_str() {
                    "exact" => {
                        println!(
                            "[OK  ] {} {} (Game: {})",
                            scanned_file.hash,
                            scanned_file.path,
                            scanned_file.game_name.expect("should have a game name")
                        );
                    }
                    "partial" => {
                        println!(
                            "[NAME] {} {} (Expected: {}, Game: {})",
                            scanned_file.hash,
                            scanned_file.path,
                            scanned_file.rom_name.expect("should have a rom name"),
                            scanned_file.game_name.expect("should have a game name")
                        );
                    }
                    _ => {
                        println!("[MISS] {} {}", scanned_file.hash, scanned_file.path);
                    }
                }
            } else {
                println!("[HASH] {} {} (Expected: {})", hash, scanned_file.path, scanned_file.hash);
            }
        } else {
            println!("[NEW ] {}", path_str);
        }
    }

    // Print entries in the database that were not found in the directory
    for db_file in db_files.values() {
        println!("[GONE] {} {}", db_file.hash, db_file.path);
    }

    Ok(())
}

// common code

fn should_skip_file(path: &Path, exclude_extensions: &[String]) -> bool {
    // Skip directories and non-files
    if !path.is_file() {
        return true;
    }

    if let Some(extension) = path.extension().and_then(|n| n.to_str()) {
        if exclude_extensions.contains(&extension.to_owned()) {
            return true;
        }
    } else {
        // Skip files with strange extensions
        return true;
    }

    if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
        // Skip hidden files
        if filename.starts_with('.') {
            return true;
        }
    } else {
        // Skip files with strange names
        return true;
    }

    let path = path.to_str();
    // Skip files with strange paths
    path.is_none()
}

fn read_and_hash_file(path: &Path, method: HashMethod, debug: bool) -> Result<String> {
    // Read file contents
    let mut file = fs::File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    // Calculate hash
    let hash = calculate_hash(&buffer, method)?;
    debug_log!(debug, "Calculated {} hash: {} for file: {}", method, hash, path.display());

    Ok(hash)
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

fn match_roms(
    db: &database::Database,
    args: &ScanArgs,
    debug: bool,
    base_path: &str,
    path: &Path,
    hash: &str,
    found_games: &mut BTreeMap<String, GameStatus>,
) -> Result<(), anyhow::Error> {
    let filename = path
        .file_name()
        .ok_or_else(|| anyhow!("Invalid file name"))?
        .to_str()
        .ok_or_else(|| anyhow!("error converting filename to string"))?;
    let path = path.to_str().ok_or_else(|| anyhow!("Invalid path"))?;
    let hash_method: &str = args.method.into();

    let mut criteria = HashMap::new();
    criteria.insert(hash_method, hash);

    let results = db.search_roms(&criteria, &HashMap::new())?;
    let mut scanned_file = models::ScannedFile {
        base_path: base_path.to_owned(),
        path: path.to_owned(),
        hash: hash.to_owned(),
        hash_type: args.method.to_string(),
        match_type: String::from("miss"),
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
        let matches = check_rom_matches(db, args, debug, filename, &results, found_games)?;
        handle_rom_matches(db, args, debug, filename, &mut scanned_file, &matches)?;
    }
    Ok(())
}

struct Matches {
    exact: Vec<(String, String)>,
    partial: Vec<(String, String)>,
}

fn check_rom_matches(
    db: &database::Database,
    args: &ScanArgs,
    debug: bool,
    filename: &str,
    results: &Vec<(models::Game, Vec<models::Rom>)>,
    found_games: &mut BTreeMap<String, GameStatus>,
) -> Result<Matches> {
    let mut exact_matches = Vec::new();
    let mut partial_matches = Vec::new();

    for (game, roms) in results {
        let game_status = get_game_status(db, found_games, &game.name);
        for rom in roms {
            if debug {
                debug_log!(debug, "Comparing with database entry:");
                debug_log!(debug, "  Game: {}", game.name);
                debug_log!(debug, "  ROM: {}", rom.name);
                debug_log!(debug, "  Size: {}", rom.size);
                match args.method {
                    HashMethod::Crc => {
                        if let Some(h) = &rom.crc {
                            debug_log!(debug, "  CRC: {}", h);
                        }
                    }
                    HashMethod::Md5 => {
                        if let Some(h) = &rom.md5 {
                            debug_log!(debug, "  MD5: {}", h);
                        }
                    }
                    HashMethod::Sha1 => {
                        if let Some(h) = &rom.sha1 {
                            debug_log!(debug, "  SHA1: {}", h);
                        }
                    }
                }
            }

            if rom.name == filename {
                debug_log!(debug, "Found exact match");
                game_status.exact_matches.insert(filename.to_owned());
                exact_matches.push((game.name.clone(), rom.name.clone()));
            } else {
                partial_matches.push((game.name.clone(), rom.name.clone()));
                game_status
                    .partial_matches
                    .entry(rom.name.clone())
                    .or_default()
                    .insert(filename.to_owned());
            }
        }
    }
    Ok(Matches {
        exact: exact_matches,
        partial: partial_matches,
    })
}

fn get_game_status<'a>(
    db: &database::Database,
    game_status: &'a mut BTreeMap<String, GameStatus>,
    game_name: &str,
) -> &'a mut GameStatus {
    game_status.entry(game_name.to_owned()).or_insert_with(|| {
        let games = db
            .search_by_game_name(game_name, false)
            .expect("Game could not be found in database");
        let game = games.first().expect("Game could not be found in database");
        GameStatus {
            roms: game.roms.clone(),
            exact_matches: HashSet::new(),
            partial_matches: HashMap::new(),
        }
    })
}

fn handle_rom_matches(
    db: &database::Database,
    args: &ScanArgs,
    debug: bool,
    filename: &str,
    scanned_file: &mut models::ScannedFile,
    matches: &Matches,
) -> Result<()> {
    if matches.exact.len() > 0 {
        // If we have exact matches, print all of them and ignore partial matches
        for (game_name, rom_name) in &matches.exact {
            if args.file_display.contains(&DisplayMethod::Exact) {
                println!("[OK  ] {} {} (Game: {})", scanned_file.hash, filename, game_name);
            }

            update_scanned(scanned_file, "exact", game_name, rom_name);
            db.store_file(scanned_file)?;
            if args.first_match {
                return Ok(());
            }
        }
    } else {
        if matches.partial.len() > 1 {
            if args.file_display.contains(&DisplayMethod::Partial) {
                println!("[NAME] {} {}. (Multiple matches)", scanned_file.hash, filename);
            }
            // If we only have partial matches, print all of them
            for (game_name, rom_name) in &matches.partial {
                if args.file_display.contains(&DisplayMethod::Partial) {
                    println!("[NAME]   {} {} (Game: {})", scanned_file.hash, rom_name, game_name);
                }

                update_scanned(scanned_file, "partial", game_name, rom_name);
                db.store_file(scanned_file)?;
            }
        } else if matches.partial.len() == 1 {
            let (game_name, rom_name) = matches
                .partial
                .first()
                .ok_or_else(|| anyhow!("should have a partial match"))?;
            if args.rename {
                let mut new_pathname = args.directory.clone();
                new_pathname.push(rom_name);
                debug_log!(debug, "Renaming file from: {} to: {}", scanned_file.path, new_pathname.display());
                fs::rename(&scanned_file.path, new_pathname)?;
                if args.file_display.contains(&DisplayMethod::Exact) {
                    println!("[OK  ] {} {} (Game: {})", scanned_file.hash, rom_name, game_name);
                }

                update_scanned(scanned_file, "exact", game_name, rom_name);
                db.store_file(scanned_file)?;
            } else {
                if args.file_display.contains(&DisplayMethod::Partial) {
                    println!("[NAME]   {} {} (Expected: {} Game: {})", scanned_file.hash, filename, rom_name, game_name);
                }

                update_scanned(scanned_file, "partial", game_name, rom_name);
                db.store_file(scanned_file)?;
            }
        }
    }
    Ok(())
}

fn update_scanned(scanned_file: &mut models::ScannedFile, match_type: &str, game_name: &str, rom_name: &str) {
    scanned_file.match_type = match_type.to_owned();
    scanned_file.game_name = Some(game_name.to_owned());
    scanned_file.rom_name = Some(rom_name.to_owned());
}

fn print_found_games(found_games: &BTreeMap<String, GameStatus>) {
    println!("\nFound Games:");
    for (game_name, status) in found_games {
        let exact_count = status.exact_matches.len();
        let partial_count = status.partial_matches.len();
        let total_count = exact_count + partial_count;

        //only count the game as matched if we have at least one exact match or all the roms are matched
        if exact_count > 0 || total_count == status.roms.len() {
            if exact_count == status.roms.len() {
                println!("[FULL] {}", game_name);
            } else {
                println!("[PART] {} ({} exact matches, {} partial matches. {} missing)", game_name, exact_count, partial_count, status.roms.len() - (exact_count + partial_count));
                for (expected, partial_match) in &status.partial_matches {
                    for filename in partial_match {
                        println!("[NAME]   {} (Expected: {})", filename, expected);
                    }
                }
                for rom in &status.roms {
                    if !status.exact_matches.contains(&rom.name) && !status.partial_matches.contains_key(&rom.name) {
                        println!("[MISS]   {}", rom.name);
                    }
                }
            }
        }
    }
}
