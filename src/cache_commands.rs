use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use clap::Subcommand;

use crate::{cache, models, xml_parser};

#[derive(Subcommand)]
pub enum CacheCommands {
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
}

pub fn handle_command(cache_path: &Utf8Path, _debug: bool, command: &CacheCommands) -> Result<()> {
    match command {
        CacheCommands::Initialize { input } => {
            let mut cache = cache::Cache::new();
            let data = xml_parser::parse_file(input)?;
            cache.merge_data(&data)?;
            cache.save_file(cache_path)?;
        }
        CacheCommands::Import { input } => {
            let mut cache = cache::Cache::new();
            cache.load_file(cache_path)?;
            let data = xml_parser::parse_file(input)?;
            cache.merge_data(&data)?;
            cache.save_file(cache_path)?;
        }
    }
    Ok(())
}
