use crate::models::DataFile;
use anyhow::Result;
use camino::Utf8Path;
use serde_xml_rs::de::from_reader;
use std::fs::File;

pub fn parse_file(path: &Utf8Path) -> Result<DataFile> {
    let file = File::open(path)?;
    let data: DataFile = from_reader(file)?;
    Ok(data)
}
