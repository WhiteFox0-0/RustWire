use serde_bytes::ByteBuf;
use serde_derive::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::fs;

use crate::error::TorrentError;

pub trait FromBencode: Sized {
    fn from_bencode(bencode: &[u8]) -> Result<Self, TorrentError>;
}

#[derive(Serialize, Deserialize, PartialEq)]
pub struct Metainfo {
    #[serde(rename = "announce-list")]
    pub announce_list: Option<Vec<Vec<String>>>, // BEP 12, optional
    pub announce: String,

    #[serde(rename = "created by")]
    pub created_by: Option<String>,

    #[serde(rename = "creation date")]
    pub creation_date: Option<i64>,
    pub comment: Option<String>,

    pub info: Info,

    #[serde(skip)]
    pub info_hash: [u8; 20], // Not included in bencode, filled after parsing
}

impl FromBencode for Metainfo {
    fn from_bencode(bencode: &[u8]) -> Result<Self, TorrentError> {
        let mut metainfo: Metainfo = serde_bencode::from_bytes(bencode)?;
        metainfo.info_hash = compute_info_hash(bencode)?;
        Ok(metainfo)
    }
}

#[derive(Serialize, Deserialize, PartialEq)]
pub struct Info {
    pub name: String,

    #[serde(rename = "piece length")]
    pub piece_length: u64,
    pub pieces: ByteBuf,

    pub length: Option<u64>,
    pub files: Option<Vec<TorrentFile>>,

    pub private: Option<i64>,
}

#[derive(Serialize, Deserialize, PartialEq)]
pub struct TorrentFile {
    pub length: u64,
    pub path: Vec<String>,
}

pub fn parse_torrent(file: &str) -> Result<Metainfo, TorrentError> {
    let file = fs::read(file).map_err(TorrentError::Io)?;
    let metainfo = Metainfo::from_bencode(&file)?;

    Ok(metainfo)
}

fn compute_info_hash(bencode: &[u8]) -> Result<[u8; 20], TorrentError> {
    let value: serde_bencode::value::Value = serde_bencode::from_bytes(bencode)?;
    let serde_bencode::value::Value::Dict(dict) = value else {
        return Err(TorrentError::NotADictionary);
    };
    let info_value = dict.get(&b"info"[..]).ok_or(TorrentError::MissingInfoKey)?;
    let info_bytes = serde_bencode::to_bytes(info_value)?;

    let mut hasher = Sha1::new();
    hasher.update(&info_bytes);
    Ok(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SINGLE_FILE_TORRENT: &[u8] =
        b"d8:announce12:http://x.com4:infod6:lengthi100e4:name8:test.txt12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaaee";

    const MULTI_FILE_TORRENT: &[u8] =
        b"d8:announce12:http://x.com4:infod5:filesld6:lengthi10e4:pathl5:a.txteed6:lengthi20e4:pathl3:dir5:b.txteee4:name7:mydir4212:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaaee";

    #[test]
    fn decodes_announce_and_basic_info_fields() {
        let metainfo = Metainfo::from_bencode(SINGLE_FILE_TORRENT).expect("should decode");

        assert_eq!(metainfo.announce, "http://x.com");
        assert_eq!(metainfo.info.name, "test.txt");
        assert_eq!(metainfo.info.piece_length, 16384);
        assert_eq!(metainfo.info.pieces.len(), 20);
    }

    #[test]
    fn decodes_single_file_length() {
        let metainfo = Metainfo::from_bencode(SINGLE_FILE_TORRENT).expect("should decode");

        assert_eq!(metainfo.info.length, Some(100));
        assert!(metainfo.info.files.is_none());
    }

    #[test]
    fn decodes_multi_file_list() {
        let metainfo = Metainfo::from_bencode(MULTI_FILE_TORRENT).expect("should decode");

        assert!(metainfo.info.length.is_none());
        let files = metainfo.info.files.expect("expected files");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].length, 10);
        assert_eq!(files[0].path, vec!["a.txt".to_string()]);
        assert_eq!(files[1].length, 20);
        assert_eq!(files[1].path, vec!["dir".to_string(), "b.txt".to_string()]);
    }

    #[test]
    fn computes_info_hash() {
        let metainfo = Metainfo::from_bencode(SINGLE_FILE_TORRENT).expect("should decode");

        // info_hash should be populated, not left as the zeroed default
        assert_ne!(metainfo.info_hash, [0u8; 20]);

        // and should match an independent computation over the same bytes
        let expected = compute_info_hash(SINGLE_FILE_TORRENT).expect("should hash");
        assert_eq!(metainfo.info_hash, expected);
    }

    #[test]
    fn info_hash_differs_for_different_torrents() {
        let a = Metainfo::from_bencode(SINGLE_FILE_TORRENT).unwrap();
        let b = Metainfo::from_bencode(MULTI_FILE_TORRENT).unwrap();
        assert_ne!(a.info_hash, b.info_hash);
    }

    #[test]
    fn optional_top_level_fields_absent_are_none() {
        let metainfo = Metainfo::from_bencode(SINGLE_FILE_TORRENT).expect("should decode");

        assert!(metainfo.announce_list.is_none());
        assert!(metainfo.created_by.is_none());
        assert!(metainfo.creation_date.is_none());
        assert!(metainfo.comment.is_none());
    }

    #[test]
    fn rejects_malformed_bencode() {
        let result = Metainfo::from_bencode(b"not bencode");
        assert!(matches!(result, Err(TorrentError::Bencode(_))));
    }

    #[test]
    fn rejects_empty_input() {
        let result = Metainfo::from_bencode(b"");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_missing_required_field() {
        // valid dict, but info dict is missing required "pieces"
        let raw = b"d8:announce13:http://x.com4:infod6:lengthi100e4:name8:test.txt12:piece lengthi16384eee";
        let result = Metainfo::from_bencode(raw);
        assert!(result.is_err());
    }
}
