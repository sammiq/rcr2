use serde::Deserialize;
use strum::{Display, EnumString, IntoStaticStr};

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
    #[serde(rename = "@name")]
    pub name: String,
    pub description: String,
    #[serde(rename = "rom")]
    pub roms: Vec<Rom>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Rom {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@size")]
    pub size: i64,
    #[serde(rename = "@crc")]
    pub crc: Option<String>,
    #[serde(rename = "@md5")]
    pub md5: Option<String>,
    #[serde(rename = "@sha1")]
    pub sha1: Option<String>,
}

#[derive(Copy, Clone, Debug, Display, PartialEq, EnumString, IntoStaticStr)]
pub enum MatchType {
    Exact,
    Partial,
    None,
}

#[derive(Copy, Clone, Debug, Display, PartialEq, EnumString, IntoStaticStr)]
pub enum HashType {
    #[strum(ascii_case_insensitive)]
    Crc,
    #[strum(ascii_case_insensitive)]
    Md5,
    #[strum(ascii_case_insensitive)]
    Sha1,
}

// Define the ScannedFile struct
#[derive(Clone, Debug)]
pub struct ScannedFile {
    pub base_path: String,
    pub path: String,
    pub hash: String,
    pub hash_type: HashType,
    pub match_type: MatchType,
    pub game_name: Option<String>,
    pub rom_name: Option<String>,
}
