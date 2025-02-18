use crate::models::DataFile;
use anyhow::Result;
use serde_xml_rs::de::from_reader;
use std::fs::File;
use std::path::Path;

pub fn parse_file(path: &Path) -> Result<DataFile> {
    let file = File::open(path)?;
    let data: DataFile = from_reader(file)?;
    Ok(data)
}
