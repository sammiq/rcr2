use crate::models::DataFile;
use anyhow::Result;
use camino::Utf8Path;
use quick_xml::de::from_reader;
use std::{fs::File, io::BufReader};

pub fn parse_file(path: &Utf8Path) -> Result<DataFile> {
    let file = File::open(path)?;
    let data: DataFile = from_reader(BufReader::new(file))?;
    Ok(data)
}
