use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use crc32fast::Hasher;
use md5::Md5;
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, DirEntry};
use std::io::Read;
use std::path::{Path, PathBuf};
use strum::{Display, IntoStaticStr};
use zip::ZipArchive;

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
    /// Update files in the database from the directory, checking for new, renamed and removed files
    Update(ScanArgs),
    /// Check all files in the directory against the database
    Check {
        /// Directory to scan (defaults to current directory)
        #[arg(short, long, default_value = ".")]
        directory: PathBuf,

        /// Scan for files recursively
        #[arg(short, long)]
        recursive: bool,
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

    /// Stop after first exact match for each file
    #[arg(short, long, default_value = "false")]
    first_match: bool,

    /// Ignore partial matches if there are exact matches
    #[arg(short, long, default_value = "true")]
    ignore_partial: bool,

    /// Directory to scan (defaults to current directory)
    #[arg(short, long, default_value = ".")]
    directory: PathBuf,

    /// Fix the name of files if an unambiguous match is found
    #[arg(short, long)]
    fix: bool,

    /// Scan for files recursively
    #[arg(short, long)]
    recursive: bool,
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
    exact_matches: HashMap<String, HashSet<String>>,
    partial_matches: HashMap<String, HashSet<String>>,
}

pub fn handle_command(
    db: &mut database::Database,
    debug: bool,
    command: &mut FileCommands,
    exclude_extensions: &[String],
) -> Result<()> {
    match command {
        FileCommands::Scan(args) => {
            args.directory = resolve_directory(&args.directory)?;
            scan_directory(db, args, debug, exclude_extensions).context("Failed to scan directory")?;
        }
        FileCommands::Update(args) => {
            args.directory = resolve_directory(&args.directory)?;
            update_directory(db, args, debug, exclude_extensions).context("Failed to update directory")?;
        }
        FileCommands::Check { directory, recursive: _ } => {
            let directory = resolve_directory(directory)?;
            check_directory(db, &directory, debug, exclude_extensions).context("Failed to check directory")?;
        }
    }
    Ok(())
}

fn resolve_directory(directory: &Path) -> Result<PathBuf> {
    if !directory.exists() {
        return Err(anyhow!("Directory does not exist: {}", directory.display()));
    }
    if !directory.is_dir() {
        return Err(anyhow!("Not a directory: {}", directory.display()));
    }
    directory.canonicalize().context("Failed to resolve directory to full path")
}

// scan functions

fn scan_directory(db: &database::Database, args: &ScanArgs, debug: bool, exclude_extensions: &[String]) -> Result<()> {
    let hash_method: &str = args.method.into();
    debug_log!(debug, "Using hash type: {}", hash_method);

    let mut found_games: BTreeMap<String, GameStatus> = BTreeMap::new();

    let mut dir_stack: Vec<PathBuf> = Vec::new();
    dir_stack.push(args.directory.clone());

    while let Some(current_path) = dir_stack.pop() {
        let current_dir = current_path.to_str().ok_or_else(|| anyhow!("Invalid base path"))?;
        println!("Scanning directory: {}", current_dir);

        // Read directory contents and sort by path
        let mut entries: Vec<_> = fs::read_dir(&current_path)?.filter_map(Result::ok).collect();
        entries.sort_by_key(DirEntry::path);

        //before we start scanning the directory, we need to clear the database of any files that have the same base path
        db.clear_files_by_base_path(current_dir)?;

        for entry in entries {
            let full_path = entry.path();

            if full_path.is_dir() {
                if args.recursive {
                    debug_log!(debug, "\nDebug: Queuing directory: {}", full_path.display());
                    dir_stack.push(full_path.clone());
                }
                continue;
            }

            if should_skip_file(&full_path, exclude_extensions) {
                continue;
            }

            let rel_path = full_path
                .strip_prefix(&args.directory)
                .expect("should be able to strip prefix");

            if is_zip_file(&full_path) {
                if let Err(e) =
                    scan_zip_contents(db, args, debug, current_dir, &full_path, rel_path, exclude_extensions, &mut found_games)
                {
                    //continue to next file if we have an error
                    eprintln!("Failed to process ZIP file: {}", e);
                }
                continue;
            }

            if let Err(e) = fs::File::open(&full_path)
                .context("Unable to open file")
                .and_then(|mut file| {
                    scan_file_contents(db, args, debug, current_dir, &full_path, rel_path, &mut file, &mut found_games, true)
                })
            {
                //continue to next file if we have an error
                eprintln!("Failed to process file: {}", e);
            }
        }
    }

    print_found_games(&found_games);

    Ok(())
}

fn scan_zip_contents(
    db: &database::Database,
    args: &ScanArgs,
    debug: bool,
    base_path: &str,
    zip_path: &Path,
    rel_zip_path: &Path,
    exclude_extensions: &[String],
    found_games: &mut BTreeMap<String, GameStatus>,
) -> Result<()> {
    let zip_file = fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(zip_file)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.is_dir() {
            continue;
        }

        if let Some(inner_path) = file.enclosed_name() {
            if let Some(extension) = inner_path.extension().and_then(|n| n.to_str()) {
                if exclude_extensions.contains(&extension.to_owned()) {
                    continue;
                }
            }

            let file_path = zip_path.join(&inner_path);
            let rel_file_path = rel_zip_path.join(&inner_path);
            if let Err(e) =
                scan_file_contents(db, args, debug, base_path, &file_path, &rel_file_path, &mut file, found_games, false)
            {
                //continue to next file if we have an error
                eprintln!("Failed to process file: {}", e);
            }
        }
    }
    Ok(())
}

fn scan_file_contents(
    db: &database::Database,
    args: &ScanArgs,
    debug: bool,
    current_dir: &str,
    full_file_path: &Path,
    rel_file_path: &Path,
    file: &mut impl Read,
    found_games: &mut BTreeMap<String, GameStatus>,
    can_rename: bool,
) -> Result<String> {
    debug_log!(debug, "\nDebug: Processing file: {}", rel_file_path.display());
    let hash = read_and_hash(file, args.method)?;

    let rel_path_str = rel_file_path.to_str().ok_or_else(|| anyhow!("Invalid path"))?;
    let filename = full_file_path
        .file_name()
        .ok_or_else(|| anyhow!("Invalid file name"))?
        .to_str()
        .ok_or_else(|| anyhow!("error converting filename to string"))?;
    let full_path_str = full_file_path.to_str().ok_or_else(|| anyhow!("Invalid path"))?;

    let hash_method: &str = args.method.into();

    let mut criteria = HashMap::new();
    criteria.insert(hash_method, hash.as_str());

    let results = db.search_roms(&criteria, &HashMap::new())?;
    let mut scanned_file = models::ScannedFile {
        base_path: current_dir.to_owned(), // base path is the current directory we are scanning
        path: full_path_str.to_owned(),    // full path is the full path to the file from file system root
        hash: hash.to_owned(),
        hash_type: args.method.to_string(),
        match_type: String::from("miss"),
        game_name: None,
        rom_name: None,
    };
    if results.is_empty() {
        debug_log!(debug, "No matches found in database");
        if args.file_display.contains(&DisplayMethod::Miss) {
            println!("[MISS] {} {}", hash, rel_path_str);
        }
        db.store_file(&scanned_file)?;
    } else {
        debug_log!(debug, "Found {} matching entries in database", results.len());
        let matches = check_rom_matches(db, args, debug, rel_path_str, filename, &results, found_games)?;
        handle_rom_matches(db, args, debug, full_file_path, rel_path_str, &mut scanned_file, &matches, can_rename)?;
    }
    Ok(hash)
}

// update functions

fn update_directory(db: &database::Database, args: &ScanArgs, debug: bool, exclude_extensions: &[String]) -> Result<()> {
    let hash_method: &str = args.method.into();
    debug_log!(debug, "Using hash type: {}", hash_method);

    let mut dir_stack: Vec<PathBuf> = Vec::new();
    dir_stack.push(args.directory.clone());

    let mut found_games: BTreeMap<String, GameStatus> = BTreeMap::new(); 

    let mut db_files = BTreeMap::new();
    let mut hash_to_file: BTreeMap<String, HashSet<String>> = BTreeMap::new();

    while let Some(current_path) = dir_stack.pop() {
        let current_dir = current_path.to_str().ok_or_else(|| anyhow!("Invalid base path"))?;
        println!("Updating directory: {}", current_dir);

        // Get all entries in the database with the same base path
        let files = db.get_files_by_base_path(current_dir)?;
        for file in files {
            db_files.insert(file.path.clone(), file);
        }

        // Read directory contents and sort by path
        let mut entries: Vec<_> = fs::read_dir(&current_path)?.filter_map(Result::ok).collect();
        entries.sort_by_key(DirEntry::path);

        for entry in entries {
            let full_path = entry.path();

            if full_path.is_dir() {
                if args.recursive {
                    debug_log!(debug, "\nDebug: Queuing directory: {}", full_path.display());
                    dir_stack.push(full_path.clone());
                }
                continue;
            }

            if should_skip_file(&full_path, exclude_extensions) {
                continue;
            }

            //relative path from start of scan
            let rel_file_path = full_path.strip_prefix(&args.directory).expect("should be able to strip prefix");
            debug_log!(debug, "\nDebug: Processing file: {}", rel_file_path.display());

            //check if this is a zip file and treat it accorgingly
            if is_zip_file(&full_path) {
                if let Err(e) = update_zip_contents(
                    db,
                    args,
                    debug,
                    current_dir,
                    &full_path,
                    rel_file_path,
                    exclude_extensions,
                    &mut db_files,
                    &mut hash_to_file,
                    &mut found_games,
                ) {
                    //continue to next file if we have an error
                    eprintln!("Failed to process ZIP file: {}", e);
                }
                continue;
            }

            let path_str = full_path.to_str().expect("should have a unicode path");

            if let Some(scanned_file) = db_files.remove(path_str) {
                //just treat the database as correct, and add it to the game status without recalculating the hash
                let rel_path_str = rel_file_path.to_str().ok_or_else(|| anyhow!("Invalid path"))?;
                update_found_file(db, rel_path_str, scanned_file, &mut found_games);
            } else {
                match fs::File::open(&full_path).context("Unable to open file").and_then(|mut file| {
                    scan_file_contents(db, args, debug, current_dir, &full_path, rel_file_path, &mut file, &mut found_games, true)
                }) {
                    Ok(hash) => {
                        //store the file and the hash in a hash table so that we can find renamed files
                        hash_to_file.entry(hash.clone()).or_default().insert(path_str.to_owned());
                    }
                    Err(e) => {
                        eprintln!("Failed to process file: {}", e);
                    }
                }
            }
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

fn update_zip_contents(
    db: &database::Database,
    args: &ScanArgs,
    debug: bool,
    current_dir: &str,
    zip_path: &Path,
    rel_zip_path: &Path,
    exclude_extensions: &[String],
    db_files: &mut BTreeMap<String, models::ScannedFile>,
    hash_to_file: &mut BTreeMap<String, HashSet<String>>,
    found_games: &mut BTreeMap<String, GameStatus>,
) -> Result<()> {
    let file = fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.is_dir() {
            continue;
        }

        if let Some(inner_path) = file.enclosed_name() {
            if let Some(extension) = inner_path.extension().and_then(|n| n.to_str()) {
                if exclude_extensions.contains(&extension.to_owned()) {
                    continue;
                }
            }

            debug_log!(debug, "\nDebug: Processing zip entry: {}", inner_path.display());

            let file_path = zip_path.join(&inner_path);
            let path_str = file_path.to_str().expect("should have a unicode path");
            let rel_file_path = rel_zip_path.join(&inner_path);
            let rel_path_str = rel_file_path.to_str().expect("should have a unicode path");

            if let Some(scanned_file) = db_files.remove(path_str) {
                //just treat the database as correct, and add it to the game status
                update_found_file(db, rel_path_str, scanned_file, found_games);
            } else {
                //doesn't seem to be in the database, so check the hash and add it to the database
                match scan_file_contents(db, args, debug, current_dir, &file_path, &rel_file_path, &mut file, found_games, false) {
                    Ok(hash) => {
                        //store the file and the hash in a hash table so that we can find renamed files
                        hash_to_file.entry(hash.clone()).or_default().insert(path_str.to_owned());
                    }
                    Err(e) => {
                        eprintln!("Failed to process file: {}", e);
                    }
                }
            }
        }
    }
    Ok(())
}

fn update_found_file(
    db: &database::Database,
    rel_path_str: &str,
    scanned_file: models::ScannedFile,
    found_games: &mut BTreeMap<String, GameStatus>,
) {
    if let Some(game_name) = scanned_file.game_name {
        let game_status = get_game_status(db, found_games, &game_name);
        let rom_name = scanned_file.rom_name.expect("should have a rom name if there is a game name");
        if scanned_file.match_type == "exact" {
            game_status
                .exact_matches
                .entry(rom_name)
                .or_default()
                .insert(rel_path_str.to_owned());
        } else {
            game_status
                .partial_matches
                .entry(rom_name)
                .or_default()
                .insert(rel_path_str.to_owned());
        }
    }
}

// check functions

fn check_directory(db: &database::Database, directory: &PathBuf, debug: bool, exclude_extensions: &[String]) -> Result<()> {
    let base_path = directory.to_str().ok_or_else(|| anyhow!("Invalid base path"))?;

    //FIXME recursive handling
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

        let rel_path = path.strip_prefix(directory).expect("should be able to strip prefix");
        debug_log!(debug, "\nDebug: Processing file: {}", rel_path.display());

        if is_zip_file(&path) {
            if let Err(e) = check_zip_file(debug, &path, exclude_extensions, &mut db_files) {
                //continue to next file if we have an error
                eprintln!("Failed to process ZIP file: {}", e);
            }
            continue;
        }

        let path_str = path.to_str().expect("should have a unicode path");
        if let Some(scanned_file) = db_files.remove(path_str) {
            let hash_method = HashMethod::from_str(&scanned_file.hash_type, true).expect("should always be a valid hash method");
            match fs::File::open(&path)
                .context("Unable to open file")
                .and_then(|mut file| read_and_hash(&mut file, hash_method))
            {
                Ok(hash) => {
                    print_scanned_file(hash, scanned_file);
                }
                Err(e) => {
                    eprintln!("Failed to process file: {}", e);
                }
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

fn check_zip_file(
    debug: bool,
    zip_path: &Path,
    exclude_extensions: &[String],
    db_files: &mut BTreeMap<String, models::ScannedFile>,
) -> Result<()> {
    let zip_file = fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(zip_file)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.is_dir() {
            continue;
        }

        if let Some(inner_path) = file.enclosed_name() {
            if let Some(extension) = inner_path.extension().and_then(|n| n.to_str()) {
                if exclude_extensions.contains(&extension.to_owned()) {
                    continue;
                }
            }

            debug_log!(debug, "\nDebug: Processing zip entry: {}", inner_path.display());
            let file_path = zip_path.to_path_buf().join(inner_path);
            let path_str = file_path.to_str().expect("should have a unicode path");

            if let Some(scanned_file) = db_files.remove(path_str) {
                let hash_method =
                    HashMethod::from_str(&scanned_file.hash_type, true).expect("should always be a valid hash method");
                match read_and_hash(&mut file, hash_method) {
                    Ok(hash) => {
                        print_scanned_file(hash, scanned_file);
                    }
                    Err(e) => {
                        eprintln!("Failed to process file: {}", e);
                    }
                }
            } else {
                println!("[NEW ] {}", path_str);
            }
        }
    }
    Ok(())
}

fn print_scanned_file(hash: String, scanned_file: models::ScannedFile) {
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

fn is_zip_file(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
}

fn read_and_hash(file: &mut impl Read, method: HashMethod) -> Result<String> {
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    calculate_hash(&buffer, method)
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

struct Matches {
    exact: Vec<(String, String)>,
    partial: Vec<(String, String)>,
}

fn check_rom_matches(
    db: &database::Database,
    args: &ScanArgs,
    debug: bool,
    rel_path_str: &str,
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
                debug_log!(debug, "Found exact match for file: {}", rel_path_str);
                game_status
                    .exact_matches
                    .entry(rom.name.clone())
                    .or_default()
                    .insert(rel_path_str.to_owned());
                exact_matches.push((game.name.clone(), rom.name.clone()));
            } else {
                debug_log!(debug, "Found partial match for file: {}", rel_path_str);
                partial_matches.push((game.name.clone(), rom.name.clone()));
                game_status
                    .partial_matches
                    .entry(rom.name.clone())
                    .or_default()
                    .insert(rel_path_str.to_owned());
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
            exact_matches: HashMap::new(),
            partial_matches: HashMap::new(),
        }
    })
}

fn handle_rom_matches(
    db: &database::Database,
    args: &ScanArgs,
    debug: bool,
    full_file_path: &Path,
    rel_path_str: &str,
    scanned_file: &mut models::ScannedFile,
    matches: &Matches,
    can_rename: bool,
) -> Result<()> {
    debug_log!(debug, "Checking matches for file: {}", rel_path_str);

    if !matches.exact.is_empty() {
        for (game_name, rom_name) in &matches.exact {
            if args.file_display.contains(&DisplayMethod::Exact) {
                println!("[OK  ] {} {} (Game: {})", scanned_file.hash, rel_path_str, game_name);
            }

            update_scanned(scanned_file, "exact", game_name, rom_name);
            db.store_file(scanned_file)?;
            //if this is set, don't bother printing other matches
            if args.first_match {
                return Ok(());
            }
        }
        //if this is set, don't bother with partial matches
        if args.ignore_partial {
            return Ok(());
        }
    }

    if matches.partial.len() > 1 {
        if args.file_display.contains(&DisplayMethod::Partial) {
            println!("[NAME] {} {}. (Multiple matches)", scanned_file.hash, rel_path_str);
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
        let (game_name, rom_name) = matches.partial.first().expect("should have a partial match");
        if can_rename && args.fix {
            let new_pathname = full_file_path.with_file_name(rom_name);
            debug_log!(debug, "Renaming file from: {} to: {}", scanned_file.path, new_pathname.display());
            if let Err(e) = fs::rename(&scanned_file.path, new_pathname) {
                eprintln!("Failed to rename file: {}", e);
                if args.file_display.contains(&DisplayMethod::Partial) {
                    println!("[NAME] {} {} (Expected: {} Game: {})", scanned_file.hash, rel_path_str, rom_name, game_name);
                }
    
                update_scanned(scanned_file, "partial", game_name, rom_name);
                db.store_file(scanned_file)?;
            } else {
                if args.file_display.contains(&DisplayMethod::Exact) {
                    println!("[OK  ] {} {} (Game: {})", scanned_file.hash, rom_name, game_name);
                }
    
                update_scanned(scanned_file, "exact", game_name, rom_name);
                db.store_file(scanned_file)?;
            }
        } else {
            if args.file_display.contains(&DisplayMethod::Partial) {
                println!("[NAME] {} {} (Expected: {} Game: {})", scanned_file.hash, rel_path_str, rom_name, game_name);
            }

            update_scanned(scanned_file, "partial", game_name, rom_name);
            db.store_file(scanned_file)?;
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
                for (rom_name, filenames) in &status.exact_matches {
                    if filenames.len() > 1 {
                        for filename in filenames {
                            println!("[DUPE]   {} (File: {})", rom_name, filename);
                        }
                    }
                }
            } else {
                println!(
                    "[PART] {} ({} exact matches, {} partial matches. {} missing)",
                    game_name,
                    exact_count,
                    partial_count,
                    status.roms.len() - (exact_count + partial_count)
                );
                for (expected, partial_match) in &status.partial_matches {
                    for filename in partial_match {
                        println!("[NAME]   {} (Expected: {})", filename, expected);
                    }
                }
                for rom in &status.roms {
                    if !status.exact_matches.contains_key(&rom.name) && !status.partial_matches.contains_key(&rom.name) {
                        println!("[MISS]   {}", rom.name);
                    }
                }
            }
        }
    }
}
