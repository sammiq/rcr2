use crate::models::{DataFile, Game, HashType, MatchType, Rom, ScannedFile};
use anyhow::{anyhow, Context, Result};
use camino::Utf8Path;
use rusqlite::{params, Connection};
use std::{collections::HashMap, str::FromStr};

macro_rules! debug_log {
    ($debug:expr, $($arg:tt)*) => {
        if $debug {
            eprintln!("{}", format!("Debug: {}", format!($($arg)*)));
        }
    };
}

pub struct Database {
    conn: Connection,
}

pub fn check_for_database(path: &Utf8Path, debug: bool) -> Result<Database> {
    if path.is_file() {
        debug_log!(debug, "database file {} exists, will attempt to connect", path);
        let db = Database::new(path).context("Failed to connect to database")?;
        Ok(db)
    } else {
        Err(anyhow!("Database file {} does not exist, please initialize the database first", path))
    }
}

impl Database {
    pub fn new(path: &Utf8Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Ok(Self { conn })
    }

    pub fn initialize(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;

        tx.execute(
            "CREATE TABLE IF NOT EXISTS games (
                name TEXT PRIMARY KEY,
                description TEXT NOT NULL
            )",
            [],
        )?;

        tx.execute(
            "CREATE TABLE IF NOT EXISTS roms (
                game_name TEXT NOT NULL,
                name TEXT NOT NULL,
                size INTEGER NOT NULL,
                crc TEXT,
                md5 TEXT,
                sha1 TEXT,
                PRIMARY KEY (game_name, name),
                FOREIGN KEY(game_name) REFERENCES games(name) ON DELETE CASCADE
            )",
            [],
        )?;

        tx.execute(
            "CREATE TABLE IF NOT EXISTS scanned_files (
                base_path TEXT NOT NULL,
                path TEXT PRIMARY KEY,
                hash TEXT NOT NULL,
                hash_type TEXT NOT NULL,
                match_type TEXT NOT NULL,
                game_name TEXT,
                rom_name TEXT,
                FOREIGN KEY(game_name, rom_name) REFERENCES roms(game_name, name)
            )",
            [],
        )?;

        tx.commit()?;
        Ok(())
    }

    pub fn store_file(&self, file: &ScannedFile) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO scanned_files (base_path, path, hash, hash_type, match_type, game_name, rom_name)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                file.base_path,
                file.path,
                file.hash,
                file.hash_type.to_string(),
                file.match_type.to_string(),
                file.game_name,
                file.rom_name
            ],
        )?;
        Ok(())
    }

    pub fn merge_data(&mut self, data: DataFile) -> Result<()> {
        let tx = self.conn.transaction()?;

        for game in data.games {
            tx.execute(
                "INSERT OR REPLACE INTO games (name, description) 
                 VALUES (?1, ?2)",
                params![game.name, game.description],
            )?;

            // Delete existing ROMs for this game
            tx.execute("DELETE FROM roms WHERE game_name = ?1", params![game.name])?;

            // Insert new ROMs
            for rom in game.roms {
                tx.execute(
                    "INSERT INTO roms (game_name, name, size, crc, md5, sha1) 
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![game.name, rom.name, rom.size, rom.crc, rom.md5, rom.sha1,],
                )?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    pub fn search_by_game_name(&self, name: &str, fuzzy: bool) -> Result<Vec<Game>> {
        let query = "SELECT g.name, g.description, r.name, r.size, r.crc, r.md5, r.sha1
             FROM games g
             JOIN roms r ON g.name = r.game_name";

        let condition = if fuzzy {
            format!("{} WHERE g.name LIKE ? ORDER BY g.name, r.name", query)
        } else {
            format!("{} WHERE g.name = ? ORDER BY g.name, r.name", query)
        };

        let param = if fuzzy { format!("%{}%", name) } else { name.to_owned() };

        self.fetch_games_and_roms(&condition, &[param]).map(|results| {
            let mut games: Vec<Game> = results
                .into_iter()
                .map(|(mut game, roms)| {
                    game.roms = roms;
                    game
                })
                .collect();
            games.sort_by(|a, b| a.name.cmp(&b.name));
            games
        })
    }

    pub fn search_roms(
        &self,
        criteria: &HashMap<&str, &str>,
        fuzzy_criteria: &HashMap<&str, &str>,
    ) -> Result<Vec<(Game, Vec<Rom>)>> {
        let mut conditions = Vec::new();
        let mut params: Vec<String> = Vec::new();

        for (key, value) in criteria {
            conditions.push(format!("r.{} = ?", key));
            params.push(String::from(*value));
        }

        for (key, value) in fuzzy_criteria {
            conditions.push(format!("r.{} LIKE ?", key));
            params.push(format!("%{}%", value));
        }

        let query = format!(
            "SELECT g.name, g.description, r.name, r.size, r.crc, r.md5, r.sha1
             FROM games g
             JOIN roms r ON g.name = r.game_name
             WHERE {}
             ORDER BY g.name, r.name",
            conditions.join(" AND ")
        );

        self.fetch_games_and_roms(&query, &params)
    }

    fn fetch_games_and_roms(&self, query: &str, params: &[String]) -> Result<Vec<(Game, Vec<Rom>)>> {
        let mut stmt = self.conn.prepare(query)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            Ok((
                Game {
                    name: row.get(0)?,
                    description: row.get(1)?,
                    roms: vec![],
                },
                Rom {
                    name: row.get(2)?,
                    size: row.get(3)?,
                    crc: row.get(4)?,
                    md5: row.get(5)?,
                    sha1: row.get(6)?,
                },
            ))
        })?;

        let mut games_map = HashMap::new();

        for row in rows {
            let (game, rom) = row?;
            games_map
                .entry(game.name.clone())
                .or_insert_with(|| (game, Vec::new()))
                .1
                .push(rom);
        }

        let results: Vec<_> = games_map.into_values().collect();
        Ok(results)
    }

    pub fn get_files_by_base_path(&self, base_path: &str) -> Result<Vec<ScannedFile>> {
        let mut stmt = self.conn.prepare(
            "SELECT base_path, path, hash, hash_type, match_type, game_name, rom_name
             FROM scanned_files
             WHERE base_path = ?1",
        )?;
        let rows = stmt.query_map(params![base_path], |row| {
            let raw_type: String = row.get(3)?;
            let raw_match: String = row.get(4)?;
            Ok(ScannedFile {
                base_path: row.get(0)?,
                path: row.get(1)?,
                hash: row.get(2)?,
                hash_type: HashType::from_str(&raw_type).expect("should be a valid HashType"),
                match_type: MatchType::from_str(&raw_match).expect("should be a valid MatchType"),
                game_name: row.get(5)?,
                rom_name: row.get(6)?,
            })
        })?;
        let mut scanned_files = Vec::new();
        for row in rows {
            scanned_files.push(row?);
        }
        Ok(scanned_files)
    }

    pub fn get_files_under_base_path(&self, base_path: &str) -> Result<Vec<ScannedFile>> {
        let mut stmt = self.conn.prepare(
            "SELECT base_path, path, hash, hash_type, match_type, game_name, rom_name
             FROM scanned_files
             WHERE base_path LIKE ?1",
        )?;
        let rows = stmt.query_map(params![format!("{}%", base_path)], |row| {
            let raw_type: String = row.get(3)?;
            let raw_match: String = row.get(4)?;
            Ok(ScannedFile {
                base_path: row.get(0)?,
                path: row.get(1)?,
                hash: row.get(2)?,
                hash_type: HashType::from_str(&raw_type).expect("should be a valid HashType"),
                match_type: MatchType::from_str(&raw_match).expect("should be a valid MatchType"),
                game_name: row.get(5)?,
                rom_name: row.get(6)?,
            })
        })?;
        let mut scanned_files = Vec::new();
        for row in rows {
            scanned_files.push(row?);
        }
        Ok(scanned_files)
    }

    pub fn clear_files_by_base_path(&self, base_path: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM scanned_files WHERE base_path = ?1", [base_path])?;
        Ok(())
    }

    pub fn delete_file(&self, path: &str) -> Result<()> {
        self.conn.execute("DELETE FROM scanned_files WHERE path = ?1", [path])?;
        Ok(())
    }
}
