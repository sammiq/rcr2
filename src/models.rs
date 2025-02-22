use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct DataFile {
    pub header: Header,
    #[serde(rename = "game")]
    pub games: Vec<Game>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Header {
    pub name: String,
    pub description: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Game {
    //#[serde(attribute)]
    pub name: String,
    pub category: String,
    pub description: String,
    #[serde(rename = "rom")]
    pub roms: Vec<Rom>,
}

#[derive(Clone, Debug, Deserialize)]
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

// Define the ScannedFile struct
#[derive(Clone, Debug)]
pub struct ScannedFile {
    pub base_path: String,
    pub path: String,
    pub hash: String,
    pub hash_type: String,
    pub match_type: String,
    pub game_name: Option<String>,
    pub rom_name: Option<String>,
}
