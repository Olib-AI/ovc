//! Content-defined chunking using a gear hash rolling function.
//!
//! Implements a FastCDC-style algorithm that splits data into variable-size
//! chunks whose boundaries are determined by the content itself. This means
//! insertions or deletions only affect the chunks near the change, enabling
//! efficient deduplication when syncing large `.ovc` files.

use sha2::{Digest, Sha256};

/// Parameters controlling chunk size distribution.
#[derive(Debug, Clone)]
pub struct ChunkParams {
    /// Minimum chunk size in bytes (hard lower bound).
    pub min_size: u32,
    /// Average target chunk size in bytes.
    pub avg_size: u32,
    /// Maximum chunk size in bytes (hard upper bound).
    pub max_size: u32,
}

impl Default for ChunkParams {
    fn default() -> Self {
        Self {
            min_size: 256 * 1024,  // 256 KiB
            avg_size: 1024 * 1024, // 1 MiB
            max_size: 4096 * 1024, // 4 MiB
        }
    }
}

/// A single chunk produced by content-defined chunking.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// SHA-256 hex digest of the chunk data.
    pub hash: String,
    /// Byte offset of this chunk in the original data.
    pub offset: u64,
    /// Length of this chunk in bytes.
    pub length: u64,
    /// The chunk's raw data.
    pub data: Vec<u8>,
}

/// Precomputed gear hash lookup table.
///
/// Each entry is a pseudo-random `u64` seeded deterministically so that
/// the chunking algorithm is fully reproducible.
const GEAR_TABLE: [u64; 256] = generate_gear_table();

/// Generates the gear hash lookup table at compile time.
///
/// Uses a simple LCG (linear congruential generator) seeded with a fixed
/// value to produce 256 pseudo-random `u64` entries.
const fn generate_gear_table() -> [u64; 256] {
    let mut table = [0u64; 256];
    // Seed chosen for good distribution; constants from Knuth.
    let mut state: u64 = 0x6A09_E667_F3BC_C908;
    let mut i = 0;
    while i < 256 {
        // LCG step: state = state * 6364136223846793005 + 1442695040888963407
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        table[i] = state;
        i += 1;
    }
    table
}

/// Splits `data` into content-defined chunks using a gear hash rolling function.
///
/// The algorithm scans through the data maintaining a rolling hash. When the
/// hash matches a boundary condition (low-order bits are zero), a chunk
/// boundary is declared — subject to the `min_size` and `max_size` constraints.
///
/// Two masks are used:
/// - A "large" mask (more bits set) for data below `avg_size` — makes
///   boundaries harder to trigger, biasing toward the target average.
/// - A "small" mask (fewer bits set) for data above `avg_size` — makes
///   boundaries easier to trigger, preventing very large chunks.
#[must_use]
pub fn chunk_data(data: &[u8], params: &ChunkParams) -> Vec<Chunk> {
    if data.is_empty() {
        return Vec::new();
    }

    let min = params.min_size as usize;
    let avg = params.avg_size as usize;
    let max = params.max_size as usize;

    // Compute masks from the average size.
    // Number of bits to check: log2(avg_size).
    let bits = avg.next_power_of_two().trailing_zeros();
    let mask_large = (1u64 << (bits + 1)) - 1; // harder to match
    let mask_small = (1u64 << (bits - 1)) - 1; // easier to match

    let mut chunks = Vec::new();
    let mut offset: usize = 0;

    while offset < data.len() {
        let remaining = data.len() - offset;

        // If the remaining data is at or below minimum, emit it as one chunk.
        if remaining <= min {
            let chunk_data = &data[offset..];
            chunks.push(make_chunk(chunk_data, offset as u64));
            break;
        }

        let chunk_end = find_boundary(&data[offset..], min, avg, max, mask_large, mask_small);

        let chunk_data = &data[offset..offset + chunk_end];
        chunks.push(make_chunk(chunk_data, offset as u64));
        offset += chunk_end;
    }

    chunks
}

/// Finds the next chunk boundary within `data`, returning the chunk length.
fn find_boundary(
    data: &[u8],
    min: usize,
    avg: usize,
    max: usize,
    mask_large: u64,
    mask_small: u64,
) -> usize {
    let len = data.len();
    let end = len.min(max);

    if end <= min {
        return end;
    }

    let mut hash: u64 = 0;

    // Scan from min_size to avg_size using the large mask.
    let mid = end.min(avg);
    for i in min..mid {
        hash = hash
            .wrapping_shl(1)
            .wrapping_add(GEAR_TABLE[data[i] as usize]);
        if hash & mask_large == 0 {
            return i + 1;
        }
    }

    // Scan from avg_size to max_size using the small mask.
    for i in mid..end {
        hash = hash
            .wrapping_shl(1)
            .wrapping_add(GEAR_TABLE[data[i] as usize]);
        if hash & mask_small == 0 {
            return i + 1;
        }
    }

    // No boundary found; cut at max_size (or end of data).
    end
}

/// Constructs a [`Chunk`] from raw data at a given offset.
fn make_chunk(data: &[u8], offset: u64) -> Chunk {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    let hash = hex::encode(digest);

    Chunk {
        hash,
        offset,
        length: data.len() as u64,
        data: data.to_vec(),
    }
}

/// Computes the SHA-256 hex digest of the given data.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Reassembles the original data from an ordered slice of chunk payloads.
pub fn reassemble_chunks(chunks: &[Vec<u8>]) -> Vec<u8> {
    let total: usize = chunks.iter().map(Vec::len).sum();
    let mut out = Vec::with_capacity(total);
    for chunk in chunks {
        out.extend_from_slice(chunk);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_chunk_small_data() {
        let data = b"hello world";
        let params = ChunkParams::default();
        let chunks = chunk_data(data, &params);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].data, data);
        assert_eq!(chunks[0].offset, 0);
        assert_eq!(chunks[0].length, data.len() as u64);
    }

    #[test]
    fn multiple_chunks_large_data() {
        // Generate data larger than max_size to guarantee multiple chunks.
        let params = ChunkParams {
            min_size: 64,
            avg_size: 256,
            max_size: 1024,
        };
        let data: Vec<u8> = (0..4096u32)
            .map(|i| (i.wrapping_mul(37).wrapping_add(17) & 0xFF) as u8)
            .collect();

        let chunks = chunk_data(&data, &params);
        assert!(
            chunks.len() > 1,
            "expected multiple chunks, got {}",
            chunks.len()
        );

        // Verify coverage: chunks should cover the entire input.
        let total_len: u64 = chunks.iter().map(|c| c.length).sum();
        assert_eq!(total_len, data.len() as u64);

        // Verify offsets are monotonically increasing and contiguous.
        let mut expected_offset = 0u64;
        for chunk in &chunks {
            assert_eq!(chunk.offset, expected_offset);
            expected_offset += chunk.length;
        }
    }

    #[test]
    fn reassemble_matches_original() {
        let params = ChunkParams {
            min_size: 64,
            avg_size: 256,
            max_size: 1024,
        };
        let data: Vec<u8> = (0..8192u32)
            .map(|i| (i.wrapping_mul(53).wrapping_add(7) & 0xFF) as u8)
            .collect();

        let chunks = chunk_data(&data, &params);
        let payloads: Vec<Vec<u8>> = chunks.iter().map(|c| c.data.clone()).collect();
        let reassembled = reassemble_chunks(&payloads);

        assert_eq!(reassembled, data);
    }

    #[test]
    fn deterministic_boundaries() {
        let params = ChunkParams {
            min_size: 64,
            avg_size: 256,
            max_size: 1024,
        };
        let data: Vec<u8> = (0..4096u32)
            .map(|i| (i.wrapping_mul(41).wrapping_add(3) & 0xFF) as u8)
            .collect();

        let chunks1 = chunk_data(&data, &params);
        let chunks2 = chunk_data(&data, &params);

        assert_eq!(chunks1.len(), chunks2.len());
        for (c1, c2) in chunks1.iter().zip(chunks2.iter()) {
            assert_eq!(c1.hash, c2.hash);
            assert_eq!(c1.offset, c2.offset);
            assert_eq!(c1.length, c2.length);
        }
    }

    #[test]
    fn empty_data_produces_no_chunks() {
        let chunks = chunk_data(&[], &ChunkParams::default());
        assert!(chunks.is_empty());
    }

    #[test]
    fn chunk_hashes_are_valid_sha256() {
        let data = vec![42u8; 512];
        let params = ChunkParams {
            min_size: 64,
            avg_size: 256,
            max_size: 1024,
        };
        let chunks = chunk_data(&data, &params);
        for chunk in &chunks {
            assert_eq!(chunk.hash.len(), 64, "SHA-256 hex should be 64 chars");
            assert!(chunk.hash.chars().all(|c| c.is_ascii_hexdigit()));

            // Verify hash matches data.
            let expected = sha256_hex(&chunk.data);
            assert_eq!(chunk.hash, expected);
        }
    }
}
