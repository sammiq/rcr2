use crate::models::{DataFile, Game, HashType, Rom, ScannedFile, Search, Store};
use anyhow::{anyhow, Context, Result};
use camino::Utf8Path;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs::File, rc::Rc};

macro_rules! debug_log {
    ($debug:expr, $($arg:tt)*) => {
        if $debug {
            eprintln!("{}", format!("Debug: {}", format!($($arg)*)));
        }
    };
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RomAndGame {
    pub rom: Rc<Rom>,
    pub game: Rc<Game>,
}

pub struct Cache {
    persistent_data: Vec<Rc<Game>>,              //we can rebuild all other data from this
    scanned_files: HashMap<String, ScannedFile>, // Key is file path, which should be unique

    //temporary data
    roms_and_games: Vec<RomAndGame>, //roms have may have duplicate names and hashes, so we need to use a vector
    games_by_name: HashMap<String, Rc<Game>>, // Key is game name, which should be unique
    hash_type: HashType,
    roms_by_hash: HashMap<String, Vec<RomAndGame>>, // Key is hash value, needed to find roms quickly
}

pub fn check_for_cache(path: &Utf8Path, debug: bool) -> Result<Cache> {
    if path.is_file() {
        debug_log!(debug, "Cache file {} exists, will attempt to read", path);
        let mut cache = Cache::new();
        cache.load_file(path).context("failed to load cache file")?;
        Ok(cache)
    } else {
        Err(anyhow!("Cache file {} does not exist, please initialize the cache first", path))
    }
}

impl Cache {
    pub fn new() -> Self {
        Self {
            persistent_data: Vec::new(),
            scanned_files: HashMap::new(),
            roms_and_games: Vec::new(),
            games_by_name: HashMap::new(),
            hash_type: HashType::Sha1,
            roms_by_hash: HashMap::new(),
        }
    }

    pub fn load_file(&mut self, path: &Utf8Path) -> Result<()> {
        let mut file: File = File::open(path)?;
        self.persistent_data = bincode::serde::decode_from_std_read(&mut file, bincode::config::standard())?;
        let scanned_files: Vec<ScannedFile> = bincode::serde::decode_from_std_read(&mut file, bincode::config::standard())?;
        self.scanned_files = scanned_files.into_iter().map(|file| (file.path.clone(), file)).collect();

        self.roms_and_games.clear();
        self.games_by_name.clear();

        self.rebuild_cache_files();
        Ok(())
    }

    fn rebuild_cache_files(&mut self) {
        //clear hash cache, is rebuilt later
        self.roms_by_hash.clear();

        //rebuild all other data from persistent data
        for game in &self.persistent_data {
            for rom in &game.roms {
                self.roms_and_games.push(RomAndGame {
                    rom: Rc::new(rom.clone()),
                    game: game.clone(),
                });
            }
            self.games_by_name.insert(game.name.clone(), game.clone());
        }
    }

    pub fn save_file(&self, path: &Utf8Path) -> Result<()> {
        let mut file = File::create(path)?;
        bincode::serde::encode_into_std_write(&self.persistent_data, &mut file, bincode::config::standard())?;
        let scanned_files: Vec<ScannedFile> = self.scanned_files.values().cloned().collect();
        bincode::serde::encode_into_std_write(&scanned_files, &mut file, bincode::config::standard())?;
        Ok(())
    }

    pub fn store_file(&mut self, file: &ScannedFile) -> Result<()> {
        let owned_file = file.clone();
        self.scanned_files.insert(owned_file.path.clone(), owned_file);
        Ok(())
    }

    pub fn merge_data(&mut self, data: &DataFile) -> Result<()> {
        for game in &data.games {
            self.persistent_data.push(Rc::new(game.clone()));
        }

        self.rebuild_cache_files();

        Ok(())
    }

    pub fn search_by_game_name(&self, name: &str) -> Result<Vec<Game>> {
        let mut games: Vec<Game> = Vec::new();
        if let Some(game) = self.games_by_name.get(name) {
            games.push(Game::clone(game)); //clone the Rc<Game> to Game
        }
        Ok(games)
    }

    pub fn build_hash_index(&mut self, hash_type: HashType) {
        self.hash_type = hash_type;
        self.roms_by_hash.clear();
        for rom_and_game in &self.roms_and_games {
            let hash = match hash_type {
                HashType::Crc => rom_and_game.rom.crc.clone().unwrap_or_default(),
                HashType::Md5 => rom_and_game.rom.md5.clone().unwrap_or_default(),
                HashType::Sha1 => rom_and_game.rom.sha1.clone().unwrap_or_default(),
            };
            self.roms_by_hash.entry(hash).or_default().push(rom_and_game.clone());
        }
    }

    pub fn search_by_hash(&self, hash_type: HashType, hash: &str) -> Result<Vec<(Game, Vec<Rom>)>> {
        if hash_type != self.hash_type {
            return Err(anyhow!("Hash type mismatch, expected {:?}, got {:?}", self.hash_type, hash_type));
        }
        let mut games_map: HashMap<String, (Game, Vec<Rom>)> = HashMap::new();
        for rom_and_game in self.roms_by_hash.get(hash).unwrap_or(&Vec::new()) {
            games_map
                .entry(rom_and_game.game.name.clone())
                .or_insert_with(|| (Game::clone(&rom_and_game.game), Vec::new()))
                .1
                .push(Rom::clone(&rom_and_game.rom));
        }
        let results: Vec<_> = games_map.into_values().collect();
        Ok(results)
    }
    pub fn clear_files_by_base_path(&mut self, base_path: &str) -> Result<()> {
        self.scanned_files.retain(|_, file| file.base_path != base_path);
        Ok(())
    }
}

impl Store for Cache {
    fn clear_files_by_base_path(&mut self, base_path: &str) -> Result<()> {
        self.clear_files_by_base_path(base_path)
    }

    fn store_file(&mut self, file: &ScannedFile) -> Result<()> {
        self.store_file(file)
    }
}

impl Search for Cache {
    fn search_by_game_name(&self, name: &str) -> Result<Vec<Game>> {
        self.search_by_game_name(name)
    }

    fn search_by_hash(&self, hash_type: HashType, hash: &str) -> Result<Vec<(Game, Vec<Rom>)>> {
        self.search_by_hash(hash_type, hash)
    }
}
