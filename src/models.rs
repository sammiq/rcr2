use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct DataFile {
    pub header: Header,
    #[serde(rename = "game")]
    pub games: Vec<Game>,
}

#[derive(Debug, Deserialize)]
pub struct Header {
    pub name: String,
    pub description: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct Game {
    //#[serde(attribute)]
    pub name: String,
    pub category: String,
    pub description: String,
    #[serde(rename = "rom")]
    pub roms: Vec<Rom>,
}

#[derive(Debug, Deserialize)]
pub struct Rom {
    //#[serde(attribute)]
    pub name: String,
    //#[serde(attribute)]
    pub size: i64,
    //#[serde(attribute)]
    pub crc: Option<String>,
    //#[serde(attribute)]
    pub md5: Option<String>,
    //#[serde(attribute)]
    pub sha1: Option<String>,
}
