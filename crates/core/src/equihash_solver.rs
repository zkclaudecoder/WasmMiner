// Pure Rust Equihash(200,9) solver — port of tromp's equi_miner.c
//
// Wagner's generalized birthday attack:
// 10 rounds of collision-finding across bucket tables.
// Memory: ~144MB (two tables of 4096 buckets * 658 slots * 7 words)
//
// CRITICAL LAYOUT NOTE: The C code's memory reuse scheme places each round's
// tree node at word offset r/2 within the 7-word slot. This preserves tree
// nodes from all previous rounds, which is needed for solution recovery.
// Round 0 tree at word 0, round 2 at word 1, round 4 at word 2, etc.

use blake2b_simd::{Params as Blake2bParams, State as Blake2bState};

use crate::equihash_compress::minimal_from_indices;

// Algorithm parameters for Equihash(200,9)
const WN: u32 = 200;
const WK: u32 = 9;
const DIGITBITS: u32 = WN / (WK + 1); // 20
const RESTBITS: u32 = 8;
const BUCKBITS: u32 = DIGITBITS - RESTBITS; // 12
const NBUCKETS: usize = 1 << BUCKBITS; // 4096
const SLOTBITS: u32 = RESTBITS + 1 + 1; // 10
const SLOTRANGE: usize = 1 << SLOTBITS; // 1024
const NSLOTS: usize = SLOTRANGE * 9 / 14; // 658
const SLOTMASK: u32 = (SLOTRANGE as u32) - 1;
const NRESTS: usize = 1 << RESTBITS; // 256
const XFULL: usize = 16;
const MAXSOLS: usize = 8;
const PROOFSIZE: usize = 1 << WK; // 512

const HASHESPERBLAKE: u32 = 512 / WN; // 2
const HASHOUT: usize = (HASHESPERBLAKE * WN / 8) as usize; // 50
const NHASHES: u32 = 2 * (1 << DIGITBITS); // 2^21
const NBLOCKS: u32 = (NHASHES + HASHESPERBLAKE - 1) / HASHESPERBLAKE; // 2^20

// Each slot is 7 u32 words. The tree position shifts per round.
const SLOT_WORDS: usize = 7;

/// Returns the number of hash bytes for round r.
fn hashsize(r: u32) -> u32 {
    let hashbits = WN - (r + 1) * DIGITBITS + RESTBITS;
    (hashbits + 7) / 8
}

fn hashwords(bytes: u32) -> u32 {
    (bytes + 3) / 4
}

// Tree node encoding: bucket + 2 slot IDs packed into a u32
#[inline(always)]
fn tree_from_idx(idx: u32) -> u32 {
    idx
}

#[inline(always)]
fn tree_from_bid(bid: u32, s0: u32, s1: u32) -> u32 {
    ((bid << SLOTBITS | s0) << SLOTBITS) | s1
}

#[inline(always)]
fn tree_bucketid(t: u32) -> u32 {
    t >> (2 * SLOTBITS)
}

#[inline(always)]
fn tree_slotid0(t: u32) -> u32 {
    (t >> SLOTBITS) & SLOTMASK
}

#[inline(always)]
fn tree_slotid1(t: u32) -> u32 {
    t & SLOTMASK
}

/// Layout info for a given round.
struct HtLayout {
    prevhashunits: u32,
    dunits: u32,
    prevbo: u32,
}

impl HtLayout {
    fn new(r: u32) -> Self {
        let nexthashbytes = hashsize(r);
        let nexthashunits = hashwords(nexthashbytes);
        if r == 0 {
            HtLayout {
                prevhashunits: 0,
                dunits: 0,
                prevbo: 0,
            }
        } else {
            let prevhashbytes = hashsize(r - 1);
            let prevhashunits = hashwords(prevhashbytes);
            let prevbo = prevhashunits * 4 - prevhashbytes;
            let dunits = prevhashunits - nexthashunits;
            HtLayout {
                prevhashunits,
                dunits,
                prevbo,
            }
        }
    }
}

/// Extract a byte from a u32 word in little-endian order using shift-mask.
#[inline(always)]
fn byte_from_word(word: u32, byte_idx: usize) -> u8 {
    (word >> (8 * (byte_idx & 3))) as u8
}

/// Access a byte within a hash word array using the C union layout.
/// On little-endian: bytes[n] within contiguous u32 words maps to
/// word n/4, byte position n%4 in little-endian order.
#[inline(always)]
fn hash_byte_at(table: &[u32], slot_base: usize, hash_start: usize, byte_idx: u32) -> u8 {
    let b = byte_idx as usize;
    let word = table[slot_base + hash_start + b / 4];
    byte_from_word(word, b)
}

/// Extract xhash (rest bits) from a slot0 hash (for odd rounds and digitK).
/// C: (pslot->hash->bytes[prevbo] & 0xf) << 4 | pslot->hash->bytes[prevbo+1] >> 4
#[inline(always)]
fn getxhash0(table: &[u32], slot_base: usize, hash_start: usize, prevbo: u32) -> u32 {
    let b0 = hash_byte_at(table, slot_base, hash_start, prevbo) as u32;
    let b1 = hash_byte_at(table, slot_base, hash_start, prevbo + 1) as u32;
    ((b0 & 0xf) << 4) | (b1 >> 4)
}

/// Extract xhash from a slot1 hash (for even rounds).
/// C: pslot->hash->bytes[prevbo]
#[inline(always)]
fn getxhash1(table: &[u32], slot_base: usize, hash_start: usize, prevbo: u32) -> u32 {
    hash_byte_at(table, slot_base, hash_start, prevbo) as u32
}

/// Collision tracking data structure.
struct CollisionData {
    nxhashslots: [u16; NRESTS],
    xhashslots: [[u16; XFULL]; NRESTS],
    n0: u32,
    n1: u32,
    xx: usize,
}

impl CollisionData {
    fn new() -> Self {
        CollisionData {
            nxhashslots: [0u16; NRESTS],
            xhashslots: [[0u16; XFULL]; NRESTS],
            n0: 0,
            n1: 0,
            xx: 0,
        }
    }

    fn clear(&mut self) {
        self.nxhashslots = [0u16; NRESTS];
    }

    fn addslot(&mut self, s1: u32, xh: u32) -> bool {
        let xh = xh as usize;
        self.n1 = self.nxhashslots[xh] as u32;
        self.nxhashslots[xh] += 1;
        if self.n1 >= XFULL as u32 {
            return false;
        }
        self.xhashslots[xh][self.n1 as usize] = s1 as u16;
        self.xx = xh;
        self.n0 = 0;
        true
    }

    fn nextcollision(&self) -> bool {
        self.n0 < self.n1
    }

    fn slot(&mut self) -> u32 {
        let val = self.xhashslots[self.xx][self.n0 as usize] as u32;
        self.n0 += 1;
        val
    }
}

#[inline(always)]
fn slot_base(bucket: usize, slot: usize) -> usize {
    (bucket * NSLOTS + slot) * SLOT_WORDS
}

pub struct Solver {
    table0: Vec<u32>,
    table1: Vec<u32>,
    nslots: Vec<u32>,
    sols: Vec<[u32; PROOFSIZE]>,
    nsols: usize,
}

impl Solver {
    pub fn new() -> Self {
        Solver {
            table0: vec![0u32; NBUCKETS * NSLOTS * SLOT_WORDS],
            table1: vec![0u32; NBUCKETS * NSLOTS * SLOT_WORDS],
            nslots: vec![0u32; 2 * NBUCKETS],
            sols: Vec::new(),
            nsols: 0,
        }
    }

    pub fn solve(&mut self, header: &[u8], nonce: &[u8]) -> Vec<Vec<u8>> {
        let mut personalization = Vec::from("ZcashPoW");
        personalization.extend_from_slice(&WN.to_le_bytes());
        personalization.extend_from_slice(&WK.to_le_bytes());

        let base_state = Blake2bParams::new()
            .hash_length(HASHOUT)
            .personal(&personalization)
            .to_state();

        let mut state = base_state;
        state.update(header);
        state.update(nonce);

        // Reset
        self.nslots.iter_mut().for_each(|x| *x = 0);
        self.nsols = 0;
        self.sols.clear();

        // Run algorithm — unrolled for constant-folding of HtLayout::new(r)
        self.digit0(&state);
        self.digit_odd(1);
        self.digit_even(2);
        self.digit_odd(3);
        self.digit_even(4);
        self.digit_odd(5);
        self.digit_even(6);
        self.digit_odd(7);
        self.digit_even(8);
        self.digit_k();

        let mut results = Vec::new();
        for i in 0..self.nsols.min(MAXSOLS) {
            results.push(minimal_from_indices(&self.sols[i]));
        }
        results.sort();
        results.dedup();
        results
    }

    fn getslot(&mut self, r: u32, bucketid: usize) -> u32 {
        let idx = (r & 1) as usize * NBUCKETS + bucketid;
        let s = self.nslots[idx];
        self.nslots[idx] = s + 1;
        s
    }

    fn getnslots(&mut self, r: u32, bucketid: usize) -> u32 {
        let idx = (r & 1) as usize * NBUCKETS + bucketid;
        let n = self.nslots[idx].min(NSLOTS as u32);
        self.nslots[idx] = 0;
        n
    }

    /// Round 0: generate initial hashes from BLAKE2b, fill table0.
    /// Tree at word 0, hash at words 1..7.
    fn digit0(&mut self, state: &Blake2bState) {
        let hashbytes = hashsize(0);
        let nexthashunits = hashwords(hashbytes);
        let _nextbo = nexthashunits * 4 - hashbytes; // always 0 for round 0

        for block in 0..NBLOCKS {
            let mut bstate = state.clone();
            bstate.update(&block.to_le_bytes());
            let hash_result = bstate.finalize();
            let hash = hash_result.as_bytes();

            for i in 0..HASHESPERBLAKE {
                let ph = &hash[(i * WN / 8) as usize..];

                // BUCKBITS=12, RESTBITS=8: bucketid = (ph[0] << 4) | (ph[1] >> 4)
                let bucketid = ((ph[0] as u32) << 4) | ((ph[1] as u32) >> 4);

                let slot_idx = self.getslot(0, bucketid as usize);
                if slot_idx >= NSLOTS as u32 {
                    continue;
                }

                let base = slot_base(bucketid as usize, slot_idx as usize);

                // Round 0: tree at word 0
                self.table0[base] = tree_from_idx(block * HASHESPERBLAKE + i);

                // Hash starts at word 1 (= 0/2 + 1)
                let hash_start_word = base + 1;
                // Copy hashbytes from ph[WN/8 - hashbytes..WN/8] into hash words
                let src_offset = (WN / 8 - hashbytes) as usize;
                let src = &ph[src_offset..(src_offset + hashbytes as usize)];

                // Word-aligned copy: nextbo=0 for round 0, hashbytes=24 = 6 words exactly
                for w in 0..nexthashunits as usize {
                    let si = w * 4;
                    self.table0[hash_start_word + w] = u32::from_le_bytes([
                        src[si],
                        src[si + 1],
                        src[si + 2],
                        src[si + 3],
                    ]);
                }
            }
        }
    }

    /// Odd rounds: read from table0 (prev even round), write to table1.
    fn digit_odd(&mut self, r: u32) {
        let htl = HtLayout::new(r);
        let mut cd = CollisionData::new();

        // Previous round was r-1 (even). Its tree is at word (r-1)/2, hash at (r-1)/2 + 1.
        let read_hash_start = ((r - 1) / 2 + 1) as usize;
        // Current round r (odd). Tree at word r/2, hash at r/2 + 1.
        let write_tree_pos = (r / 2) as usize;
        let write_hash_start = (r / 2 + 1) as usize;

        let prevhashunits = htl.prevhashunits as usize;
        let dunits = htl.dunits as usize;
        let prevbo = htl.prevbo as usize;

        for bucketid in 0..NBUCKETS {
            cd.clear();
            let bsize = self.getnslots(r - 1, bucketid);

            for s1 in 0..bsize {
                let base1 = slot_base(bucketid, s1 as usize);
                let xh = getxhash0(&self.table0, base1, read_hash_start, htl.prevbo);

                if !cd.addslot(s1, xh) {
                    continue;
                }

                // Cache s1's hash words — invariant across the collision loop
                let mut s1_hash = [0u32; 6];
                for i in 0..prevhashunits {
                    s1_hash[i] = self.table0[base1 + read_hash_start + i];
                }

                while cd.nextcollision() {
                    let s0 = cd.slot();
                    let base0 = slot_base(bucketid, s0 as usize);

                    // Check last hash word for full collision (cached s1)
                    if self.table0[base0 + read_hash_start + prevhashunits - 1]
                        == s1_hash[prevhashunits - 1]
                    {
                        continue;
                    }

                    // XOR bucket ID using word-level XOR + byte extraction
                    let xorbucketid = {
                        let w1 = (prevbo + 1) / 4;
                        let xw = self.table0[base0 + read_hash_start + w1] ^ s1_hash[w1];
                        let b1 = byte_from_word(xw, prevbo + 1);
                        let w2 = (prevbo + 2) / 4;
                        let b2 = if w2 == w1 {
                            byte_from_word(xw, prevbo + 2)
                        } else {
                            let xw2 = self.table0[base0 + read_hash_start + w2] ^ s1_hash[w2];
                            byte_from_word(xw2, prevbo + 2)
                        };
                        (((b1 & 0xf) as u32) << 8) | (b2 as u32)
                    };

                    let xorslot = self.getslot(r, xorbucketid as usize);
                    if xorslot >= NSLOTS as u32 {
                        continue;
                    }

                    let dst_base = slot_base(xorbucketid as usize, xorslot as usize);

                    // Write tree
                    self.table1[dst_base + write_tree_pos] =
                        tree_from_bid(bucketid as u32, s0, s1);

                    // XOR hash words using cached s1 values
                    for i in dunits..prevhashunits {
                        self.table1[dst_base + write_hash_start + i - dunits] =
                            self.table0[base0 + read_hash_start + i] ^ s1_hash[i];
                    }
                }
            }
        }
    }

    /// Even rounds: read from table1 (prev odd round), write to table0.
    fn digit_even(&mut self, r: u32) {
        let htl = HtLayout::new(r);
        let mut cd = CollisionData::new();

        let read_hash_start = ((r - 1) / 2 + 1) as usize;
        let write_tree_pos = (r / 2) as usize;
        let write_hash_start = (r / 2 + 1) as usize;
        let prevhashunits = htl.prevhashunits as usize;
        let dunits = htl.dunits as usize;
        let prevbo = htl.prevbo as usize;

        for bucketid in 0..NBUCKETS {
            cd.clear();
            let bsize = self.getnslots(r - 1, bucketid);

            for s1 in 0..bsize {
                let base1 = slot_base(bucketid, s1 as usize);
                let xh = getxhash1(&self.table1, base1, read_hash_start, htl.prevbo);

                if !cd.addslot(s1, xh) {
                    continue;
                }

                // Cache s1's hash words — invariant across the collision loop
                let mut s1_hash = [0u32; 6];
                for i in 0..prevhashunits {
                    s1_hash[i] = self.table1[base1 + read_hash_start + i];
                }

                while cd.nextcollision() {
                    let s0 = cd.slot();
                    let base0 = slot_base(bucketid, s0 as usize);

                    // Check last hash word for full collision (cached s1)
                    if self.table1[base0 + read_hash_start + prevhashunits - 1]
                        == s1_hash[prevhashunits - 1]
                    {
                        continue;
                    }

                    // Even round XOR bucket ID using word-level XOR + byte extraction
                    let xorbucketid = {
                        let w1 = (prevbo + 1) / 4;
                        let xw = self.table1[base0 + read_hash_start + w1] ^ s1_hash[w1];
                        let b1 = byte_from_word(xw, prevbo + 1);
                        let w2 = (prevbo + 2) / 4;
                        let b2 = if w2 == w1 {
                            byte_from_word(xw, prevbo + 2)
                        } else {
                            let xw2 = self.table1[base0 + read_hash_start + w2] ^ s1_hash[w2];
                            byte_from_word(xw2, prevbo + 2)
                        };
                        ((b1 as u32) << 4) | ((b2 as u32) >> 4)
                    };

                    let xorslot = self.getslot(r, xorbucketid as usize);
                    if xorslot >= NSLOTS as u32 {
                        continue;
                    }

                    let dst_base = slot_base(xorbucketid as usize, xorslot as usize);

                    self.table0[dst_base + write_tree_pos] =
                        tree_from_bid(bucketid as u32, s0, s1);

                    // XOR hash words using cached s1 values
                    for i in dunits..prevhashunits {
                        self.table0[dst_base + write_hash_start + i - dunits] =
                            self.table1[base0 + read_hash_start + i] ^ s1_hash[i];
                    }
                }
            }
        }
    }

    /// Final round (K=9, odd): find final collisions, emit candidate solutions.
    fn digit_k(&mut self) {
        let htl = HtLayout::new(WK);
        let mut cd = CollisionData::new();

        // Reading from table0, round 8 data: hash at (8/2+1) = 5
        let read_hash_start = ((WK - 1) / 2 + 1) as usize;
        let prevhashunits = htl.prevhashunits as usize;

        for bucketid in 0..NBUCKETS {
            cd.clear();
            let bsize = self.getnslots(WK - 1, bucketid);

            for s1 in 0..bsize {
                let base1 = slot_base(bucketid, s1 as usize);
                let xh = getxhash0(&self.table0, base1, read_hash_start, htl.prevbo);

                if !cd.addslot(s1, xh) {
                    continue;
                }

                // Cache s1's last hash word for the dup check
                let s1_last = self.table0[base1 + read_hash_start + prevhashunits - 1];

                while cd.nextcollision() {
                    let s0 = cd.slot();
                    let base0 = slot_base(bucketid, s0 as usize);

                    if self.table0[base0 + read_hash_start + prevhashunits - 1] == s1_last {
                        let tree = tree_from_bid(bucketid as u32, s0, s1);
                        self.candidate(tree);
                    }
                }
            }
        }
    }

    fn candidate(&mut self, t: u32) {
        // First pass: recover indices and check for duplicates (sorted)
        let mut prf = [0u32; PROOFSIZE];
        self.listindices1(WK, t, &mut prf, 0);

        prf.sort_unstable();
        for i in 1..PROOFSIZE {
            if prf[i] <= prf[i - 1] {
                return; // duplicate indices
            }
        }

        // Second pass: recover tree-ordered indices for the actual solution
        // (the verifier expects indices in tree order, not sorted)
        if self.nsols < MAXSOLS {
            let mut sol = [0u32; PROOFSIZE];
            self.listindices1(WK, t, &mut sol, 0);
            self.sols.push(sol);
        }
        self.nsols += 1;
    }

    /// Traverse tree from table0 (called for even r values during descent).
    /// C: listindices0 does --r then accesses trees1[r/2]
    fn listindices0(&self, r: u32, t: u32, indices: &mut [u32], offset: usize) {
        if r == 0 {
            indices[offset] = t;
            return;
        }
        let r = r - 1; // C: --r
        let bid = tree_bucketid(t) as usize;
        let s0 = tree_slotid0(t) as usize;
        let s1 = tree_slotid1(t) as usize;
        let size = 1usize << r;

        // Access trees1[r/2] — tree is at word position r/2 within table1 slot
        let tree_pos = (r / 2) as usize;
        let base0 = slot_base(bid, s0);
        let base1 = slot_base(bid, s1);
        let tree0 = self.table1[base0 + tree_pos];
        let tree1 = self.table1[base1 + tree_pos];

        self.listindices1(r, tree0, indices, offset);
        self.listindices1(r, tree1, indices, offset + size);
        orderindices(indices, offset, size);
    }

    /// Traverse tree from table1 (called for odd r values during descent).
    /// C: listindices1 does --r then accesses trees0[r/2]
    fn listindices1(&self, r: u32, t: u32, indices: &mut [u32], offset: usize) {
        let r = r - 1; // C: --r
        let bid = tree_bucketid(t) as usize;
        let s0 = tree_slotid0(t) as usize;
        let s1 = tree_slotid1(t) as usize;
        let size = 1usize << r;

        // Access trees0[r/2] — tree is at word position r/2 within table0 slot
        let tree_pos = (r / 2) as usize;
        let base0 = slot_base(bid, s0);
        let base1 = slot_base(bid, s1);
        let tree0 = self.table0[base0 + tree_pos];
        let tree1 = self.table0[base1 + tree_pos];

        self.listindices0(r, tree0, indices, offset);
        self.listindices0(r, tree1, indices, offset + size);
        orderindices(indices, offset, size);
    }
}

fn orderindices(indices: &mut [u32], offset: usize, size: usize) {
    if indices[offset] > indices[offset + size] {
        for i in 0..size {
            indices.swap(offset + i, offset + size + i);
        }
    }
}
