use anyhow::Result;
use rusqlite::{Connection, params};
use crate::models::{DataFile, Game, Rom};

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new(path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Ok(Self { conn })
    }
    
    pub fn initialize(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;

        tx.execute(
            "CREATE TABLE IF NOT EXISTS games (
                name TEXT PRIMARY KEY,
                category TEXT NOT NULL,
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

        tx.commit()?;
        Ok(())
    }

    pub fn merge_data(&mut self, data: DataFile) -> Result<()> {
        let tx = self.conn.transaction()?;
        
        for game in data.games {
            tx.execute(
                "INSERT OR REPLACE INTO games (name, category, description) 
                 VALUES (?1, ?2, ?3)",
                params![game.name, game.category, game.description],
            )?;

            // Delete existing ROMs for this game
            tx.execute(
                "DELETE FROM roms WHERE game_name = ?1",
                params![game.name],
            )?;

            // Insert new ROMs
            for rom in game.roms {
                tx.execute(
                    "INSERT INTO roms (game_name, name, size, crc, md5, sha1) 
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        game.name,
                        rom.name,
                        rom.size,
                        rom.crc,
                        rom.md5,
                        rom.sha1,
                    ],
                )?;
            }
        }

        tx.commit()?;
        Ok(())
    }


    pub fn search_by_game_name(&self, name: &str) -> Result<Vec<(Game, Vec<Rom>)>> {
        self.fetch_games_and_roms(
            "SELECT g.name, g.category, g.description, r.name, r.size, r.crc, r.md5, r.sha1
             FROM games g
             JOIN roms r ON g.name = r.game_name
             WHERE g.name LIKE ?",
            &[format!("%{}%", name)],
        )
    }

    pub fn search_roms(
        &self,
        name: Option<&str>,
        crc: Option<&str>,
        md5: Option<&str>,
        sha1: Option<&str>,
    ) -> Result<Vec<(Game, Vec<Rom>)>> {
        let mut conditions = Vec::new();
        let mut params = Vec::new();

        if let Some(name) = name {
            conditions.push("r.name = ?");
            params.push(name.to_string());
        }
        if let Some(crc) = crc {
            conditions.push("r.crc = ?");
            params.push(crc.to_string());
        }
        if let Some(md5) = md5 {
            conditions.push("r.md5 = ?");
            params.push(md5.to_string());
        }
        if let Some(sha1) = sha1 {
            conditions.push("r.sha1 = ?");
            params.push(sha1.to_string());
        }

        let query = format!(
            "SELECT g.name, g.category, g.description, r.name, r.size, r.crc, r.md5, r.sha1
             FROM games g
             JOIN roms r ON g.name = r.game_name
             WHERE {}",
            conditions.join(" AND ")
        );

        self.fetch_games_and_roms(&query, &params)
    }

    fn fetch_games_and_roms(&self, query: &str, params: &[String]) -> Result<Vec<(Game, Vec<Rom>)>> {
        let mut stmt = self.conn.prepare(query)?;
        let rows = stmt.query_map( rusqlite::params_from_iter(params), |row| {
            Ok((
                Game {
                    name: row.get(0)?,
                    category: row.get(1)?,
                    description: row.get(2)?,
                    roms: vec![],
                },
                Rom {
                    name: row.get(3)?,
                    size: row.get(4)?,
                    crc: row.get(5)?,
                    md5: row.get(6)?,
                    sha1: row.get(7)?,
                }
            ))
        })?;

        let mut games_map = std::collections::HashMap::new();
        
        for row in rows {
            let (game, rom) = row?;
            games_map.entry(game.name.clone())
                .or_insert_with(|| (game, Vec::new()))
                .1.push(rom);
        }

        let results: Vec<_> = games_map.into_values().collect();
        Ok(results)
    }
}