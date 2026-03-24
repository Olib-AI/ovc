//! Compression utilities for OVC object storage.
//!
//! Uses [Zstandard](https://facebook.github.io/zstd/) for high-ratio, fast
//! compression of object data before encryption.

use std::io::Read as _;

use crate::error::{CoreError, CoreResult};

/// Default Zstandard compression level (balanced speed/ratio).
pub const DEFAULT_COMPRESSION_LEVEL: i32 = 3;

/// Maximum allowed decompressed output size (256 MiB) to prevent zip-bomb style
/// attacks where a small compressed payload expands into gigabytes of memory.
const MAX_DECOMPRESSED_SIZE: usize = 256 * 1024 * 1024;

/// Compression algorithm identifier (for format header).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum CompressionAlgorithm {
    /// No compression — data is stored as-is.
    None = 0,
    /// Zstandard compression.
    Zstd = 1,
}

impl CompressionAlgorithm {
    /// Creates a `CompressionAlgorithm` from its `u8` discriminant.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Zstd),
            _ => None,
        }
    }
}

/// Compresses data using Zstandard at the given level.
///
/// Valid levels range from 1 (fastest) to 22 (best ratio). A level of 0
/// uses the Zstd default (currently 3).
pub fn compress(data: &[u8], level: i32) -> CoreResult<Vec<u8>> {
    zstd::encode_all(std::io::Cursor::new(data), level).map_err(|e| CoreError::Compression {
        reason: e.to_string(),
    })
}

/// Decompresses Zstandard-compressed data.
///
/// Enforces a maximum decompressed size of 256 MiB to guard against
/// decompression bombs. Returns [`CoreError::Compression`] if the limit
/// is exceeded.
pub fn decompress(data: &[u8]) -> CoreResult<Vec<u8>> {
    let decoder =
        zstd::Decoder::new(std::io::Cursor::new(data)).map_err(|e| CoreError::Compression {
            reason: e.to_string(),
        })?;
    let mut output = Vec::new();
    let mut limited = decoder.take(MAX_DECOMPRESSED_SIZE as u64 + 1);
    std::io::copy(&mut limited, &mut output).map_err(|e| CoreError::Compression {
        reason: e.to_string(),
    })?;
    if output.len() > MAX_DECOMPRESSED_SIZE {
        return Err(CoreError::Compression {
            reason: "decompressed size exceeds 256 MiB limit".into(),
        });
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let original = b"the quick brown fox jumps over the lazy dog. ".repeat(100);
        let compressed = compress(&original, DEFAULT_COMPRESSION_LEVEL).unwrap();
        assert!(compressed.len() < original.len());
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn empty_data() {
        let compressed = compress(b"", DEFAULT_COMPRESSION_LEVEL).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert!(decompressed.is_empty());
    }
}
