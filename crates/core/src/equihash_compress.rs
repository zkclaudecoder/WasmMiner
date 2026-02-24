#![allow(dead_code)]
// Compression/expansion utilities for Equihash(200,9) solutions.
// Ported from equihash crate's src/minimal.rs, hardcoded for (200,9).
//
// For (200,9): collision_bit_length = 20, so bit_len = 21
// PROOFSIZE = 512 indices, compressed solution = 1344 bytes

const BIT_LEN: usize = 21; // collision_bit_length + 1

fn compress_array(array: &[u8], bit_len: usize, byte_pad: usize) -> Vec<u8> {
    let index_bytes = 4usize; // size_of::<u32>()
    assert!(bit_len >= 8);
    assert!(8 * index_bytes >= 7 + bit_len);

    let in_width: usize = (bit_len + 7) / 8 + byte_pad;
    let out_len = bit_len * array.len() / (8 * in_width);

    let mut out = Vec::with_capacity(out_len);
    let bit_len_mask: u32 = (1 << (bit_len as u32)) - 1;

    let mut acc_bits: usize = 0;
    let mut acc_value: u32 = 0;

    let mut j: usize = 0;
    for _i in 0..out_len {
        if acc_bits < 8 {
            acc_value <<= bit_len;
            for x in byte_pad..in_width {
                acc_value |= ((array[j + x]
                    & ((bit_len_mask >> (8 * (in_width - x - 1))) as u8))
                    as u32)
                    .wrapping_shl(8 * (in_width - x - 1) as u32);
            }
            j += in_width;
            acc_bits += bit_len;
        }

        acc_bits -= 8;
        out.push((acc_value >> acc_bits) as u8);
    }

    out
}

fn expand_array(vin: &[u8], bit_len: usize, byte_pad: usize) -> Vec<u8> {
    assert!(bit_len >= 8);
    assert!(32usize >= 7 + bit_len);

    let out_width = (bit_len + 7) / 8 + byte_pad;
    let out_len = 8 * out_width * vin.len() / bit_len;

    if out_len == vin.len() {
        return vin.to_vec();
    }

    let mut vout: Vec<u8> = vec![0; out_len];
    let bit_len_mask: u32 = (1 << bit_len) - 1;

    let mut acc_bits = 0usize;
    let mut acc_value: u32 = 0;

    let mut j = 0usize;
    for b in vin {
        acc_value = (acc_value << 8) | u32::from(*b);
        acc_bits += 8;

        if acc_bits >= bit_len {
            acc_bits -= bit_len;
            for x in byte_pad..out_width {
                vout[j + x] = ((acc_value >> (acc_bits + (8 * (out_width - x - 1))))
                    & ((bit_len_mask >> (8 * (out_width - x - 1))) & 0xFF))
                    as u8;
            }
            j += out_width;
        }
    }

    vout
}

/// Convert 512 u32 indices into the 1344-byte compressed solution format.
pub fn minimal_from_indices(indices: &[u32]) -> Vec<u8> {
    let index_bytes = 4usize;
    let digit_bytes = (BIT_LEN + 7) / 8; // 3
    assert!(digit_bytes <= index_bytes);

    let byte_pad = index_bytes - digit_bytes; // 1

    let array: Vec<u8> = indices.iter().flat_map(|index| index.to_be_bytes()).collect();

    compress_array(&array, BIT_LEN, byte_pad)
}

/// Convert a 1344-byte compressed solution back to 512 u32 indices.
pub fn indices_from_minimal(minimal: &[u8]) -> Option<Vec<u32>> {
    // Expected length: (512 * 21) / 8 = 1344
    if minimal.len() != 1344 {
        return None;
    }

    let byte_pad = 4 - ((BIT_LEN + 7) / 8); // 1

    let expanded = expand_array(minimal, BIT_LEN, byte_pad);
    let mut ret = Vec::with_capacity(512);
    for chunk in expanded.chunks_exact(4) {
        ret.push(u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }

    Some(ret)
}
