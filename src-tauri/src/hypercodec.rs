use anyhow::{anyhow, Result};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

const MAGIC: &[u8; 4] = b"GSHK";
const VERSION: u8 = 4;

/// Fast invertible micro-transforms discovered automatically per file and saved as a reusable recipe.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransformOp {
    None = 0,
    Delta8 = 1,
    Transpose2 = 2,
    Transpose4 = 3,
    Transpose8 = 4,
    Transpose16 = 5,
    Float32Split = 6,
    BcnPlanar8 = 7,
    BcnPlanar16 = 8,
}

impl TransformOp {
    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0 => Ok(TransformOp::None),
            1 => Ok(TransformOp::Delta8),
            2 => Ok(TransformOp::Transpose2),
            3 => Ok(TransformOp::Transpose4),
            4 => Ok(TransformOp::Transpose8),
            5 => Ok(TransformOp::Transpose16),
            6 => Ok(TransformOp::Float32Split),
            7 => Ok(TransformOp::BcnPlanar8),
            8 => Ok(TransformOp::BcnPlanar16),
            _ => Err(anyhow!("Unknown transform op: {}", v)),
        }
    }
}

pub fn serialize_recipe(ops: &[TransformOp]) -> String {
    ops.iter()
        .map(|o| (*o as u8).to_string())
        .collect::<Vec<_>>()
        .join(",")
}

pub fn parse_recipe(s: &str) -> Vec<TransformOp> {
    let mut ops = Vec::new();
    for part in s.split(',') {
        if let Ok(v) = part.trim().parse::<u8>() {
            if let Ok(op) = TransformOp::from_u8(v) {
                ops.push(op);
            }
        }
    }
    if ops.is_empty() {
        ops.push(TransformOp::None);
    }
    ops
}

fn delta8_forward(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut prev = 0u8;
    for &b in input {
        out.push(b.wrapping_sub(prev));
        prev = b;
    }
    out
}

fn delta8_inverse(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut acc = 0u8;
    for &b in input {
        acc = acc.wrapping_add(b);
        out.push(acc);
    }
    out
}

fn transpose_forward(input: &[u8], stride: usize) -> Vec<u8> {
    if stride <= 1 || input.len() < stride * 2 {
        return input.to_vec();
    }
    let blocks = input.len() / stride;
    let rem_start = blocks * stride;
    let mut out = Vec::with_capacity(input.len());
    for lane in 0..stride {
        for b in 0..blocks {
            out.push(input[b * stride + lane]);
        }
    }
    out.extend_from_slice(&input[rem_start..]);
    out
}

fn transpose_inverse(input: &[u8], stride: usize) -> Vec<u8> {
    if stride <= 1 || input.len() < stride * 2 {
        return input.to_vec();
    }
    let blocks = input.len() / stride;
    let rem_start = blocks * stride;
    let mut out = vec![0u8; input.len()];
    let mut idx = 0;
    for lane in 0..stride {
        for b in 0..blocks {
            out[b * stride + lane] = input[idx];
            idx += 1;
        }
    }
    out[rem_start..].copy_from_slice(&input[rem_start..]);
    out
}

fn float32_split_forward(input: &[u8]) -> Vec<u8> {
    let aligned = transpose_forward(input, 4);
    let blocks = input.len() / 4;
    if blocks < 4 {
        return aligned;
    }
    let mut out = aligned;
    let start = blocks * 3;
    let end = blocks * 4;
    let d = delta8_forward(&out[start..end]);
    out[start..end].copy_from_slice(&d);
    out
}

fn float32_split_inverse(input: &[u8]) -> Vec<u8> {
    let blocks = input.len() / 4;
    if blocks < 4 {
        return transpose_inverse(input, 4);
    }
    let mut buf = input.to_vec();
    let start = blocks * 3;
    let end = blocks * 4;
    let inv = delta8_inverse(&buf[start..end]);
    buf[start..end].copy_from_slice(&inv);
    transpose_inverse(&buf, 4)
}

fn dds_header_len(input: &[u8]) -> Option<usize> {
    if input.len() < 128 || &input[0..4] != b"DDS " {
        return None;
    }
    if input.len() >= 148 && &input[84..88] == b"DX10" {
        Some(148)
    } else {
        Some(128)
    }
}

fn bcn_planar_forward(input: &[u8], block_size: usize) -> Vec<u8> {
    let hdr = dds_header_len(input).unwrap_or(0);
    if input.len() <= hdr + block_size * 4 {
        return input.to_vec();
    }
    let mut out = Vec::with_capacity(input.len());
    out.extend_from_slice(&input[..hdr]);
    let body = &input[hdr..];
    let mut transposed = transpose_forward(body, block_size);
    let blocks = body.len() / block_size;
    let ep_end = blocks * (block_size / 2);
    let delta_ep = delta8_forward(&transposed[..ep_end]);
    transposed[..ep_end].copy_from_slice(&delta_ep);
    out.extend_from_slice(&transposed);
    out
}

fn bcn_planar_inverse(input: &[u8], block_size: usize) -> Vec<u8> {
    let hdr = dds_header_len(input).unwrap_or(0);
    if input.len() <= hdr + block_size * 4 {
        return input.to_vec();
    }
    let mut out = Vec::with_capacity(input.len());
    out.extend_from_slice(&input[..hdr]);
    let mut body = input[hdr..].to_vec();
    let blocks = body.len() / block_size;
    let ep_end = blocks * (block_size / 2);
    let restored_ep = delta8_inverse(&body[..ep_end]);
    body[..ep_end].copy_from_slice(&restored_ep);
    let unt = transpose_inverse(&body, block_size);
    out.extend_from_slice(&unt);
    out
}

pub fn apply_op_forward(data: &[u8], op: TransformOp) -> Vec<u8> {
    match op {
        TransformOp::None => data.to_vec(),
        TransformOp::Delta8 => delta8_forward(data),
        TransformOp::Transpose2 => transpose_forward(data, 2),
        TransformOp::Transpose4 => transpose_forward(data, 4),
        TransformOp::Transpose8 => transpose_forward(data, 8),
        TransformOp::Transpose16 => transpose_forward(data, 16),
        TransformOp::Float32Split => float32_split_forward(data),
        TransformOp::BcnPlanar8 => bcn_planar_forward(data, 8),
        TransformOp::BcnPlanar16 => bcn_planar_forward(data, 16),
    }
}

pub fn apply_op_inverse(data: &[u8], op: TransformOp) -> Vec<u8> {
    match op {
        TransformOp::None => data.to_vec(),
        TransformOp::Delta8 => delta8_inverse(data),
        TransformOp::Transpose2 => transpose_inverse(data, 2),
        TransformOp::Transpose4 => transpose_inverse(data, 4),
        TransformOp::Transpose8 => transpose_inverse(data, 8),
        TransformOp::Transpose16 => transpose_inverse(data, 16),
        TransformOp::Float32Split => float32_split_inverse(data),
        TransformOp::BcnPlanar8 => bcn_planar_inverse(data, 8),
        TransformOp::BcnPlanar16 => bcn_planar_inverse(data, 16),
    }
}

pub fn discover_best_pipeline(data: &[u8], is_dds: bool) -> Vec<TransformOp> {
    if data.len() < 4096 {
        return vec![TransformOp::None];
    }

    let sample_len = data.len().min(256 * 1024);
    let sample = &data[..sample_len];

    let mut candidates: Vec<Vec<TransformOp>> = vec![
        vec![TransformOp::None],
        vec![TransformOp::Delta8],
        vec![TransformOp::Transpose2],
        vec![TransformOp::Transpose4],
        vec![TransformOp::Transpose8],
        vec![TransformOp::Transpose16],
        vec![TransformOp::Float32Split],
        vec![TransformOp::Transpose4, TransformOp::Delta8],
        vec![TransformOp::Transpose16, TransformOp::Delta8],
    ];

    if is_dds || dds_header_len(sample).is_some() {
        candidates.push(vec![TransformOp::BcnPlanar8]);
        candidates.push(vec![TransformOp::BcnPlanar16]);
    }

    let mut best_ops = vec![TransformOp::None];
    let mut best_size = usize::MAX;

    for ops in candidates {
        let mut buf = sample.to_vec();
        for &op in &ops {
            if op != TransformOp::None {
                buf = apply_op_forward(&buf, op);
            }
        }
        if let Ok(compressed) = zstd::bulk::compress(&buf, 3) {
            if compressed.len() < best_size {
                best_size = compressed.len();
                best_ops = ops;
            }
        }
    }

    best_ops
}

const SKETCH_LEN: usize = 32;

pub fn compute_minhash_sketch(path: &Path) -> Option<[u64; SKETCH_LEN]> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len < 8 * 1024 || len > 128 * 1024 * 1024 {
        return None;
    }

    let cap = (len as usize).min(256 * 1024);
    let mut buf = vec![0u8; cap];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    if n < 128 {
        return None;
    }

    let mut sketch = [u64::MAX; SKETCH_LEN];
    let step = (n / 1024).max(16);
    let mut pos = 0;
    while pos + 64 <= n {
        let h = blake3::hash(&buf[pos..pos + 64]);
        let hb = h.as_bytes();
        let h1 = u64::from_le_bytes(hb[0..8].try_into().unwrap());
        let h2 = u64::from_le_bytes(hb[8..16].try_into().unwrap());
        for i in 0..SKETCH_LEN {
            let mixed = h1
                .wrapping_add((i as u64).wrapping_mul(h2))
                .wrapping_mul(0x9E37_79B9_7F4A_7C15);
            if mixed < sketch[i] {
                sketch[i] = mixed;
            }
        }
        pos += step;
    }
    Some(sketch)
}

pub fn sketch_similarity(a: &[u64; SKETCH_LEN], b: &[u64; SKETCH_LEN]) -> f64 {
    let mut matches = 0usize;
    for i in 0..SKETCH_LEN {
        if a[i] == b[i] {
            matches += 1;
        }
    }
    matches as f64 / SKETCH_LEN as f64
}

pub fn find_similarity_pairs(
    files: &[(String, std::path::PathBuf, u64)],
) -> HashMap<String, String> {
    let sketched: Vec<(String, u64, [u64; SKETCH_LEN])> = files
        .par_iter()
        .filter_map(|(rel, abs, sz)| compute_minhash_sketch(abs).map(|sk| (rel.clone(), *sz, sk)))
        .collect();

    let mut buckets: HashMap<(u64, u64), Vec<usize>> = HashMap::new();
    for (idx, (_, _, sk)) in sketched.iter().enumerate() {
        buckets
            .entry((sk[0] & 0xFFFF, sk[1] & 0xFFFF))
            .or_default()
            .push(idx);
    }

    let mut pairs: HashMap<String, String> = HashMap::new();
    let mut used_as_ref: HashMap<String, bool> = HashMap::new();

    for group in buckets.values() {
        if group.len() < 2 {
            continue;
        }
        let ref_idx = group[0];
        let (ref_rel, ref_sz, ref_sk) = &sketched[ref_idx];
        if pairs.contains_key(ref_rel) {
            continue;
        }
        for &cand_idx in &group[1..] {
            let (cand_rel, cand_sz, cand_sk) = &sketched[cand_idx];
            if pairs.contains_key(cand_rel) || used_as_ref.contains_key(cand_rel) {
                continue;
            }
            let max_s = (*ref_sz).max(*cand_sz) as f64;
            let min_s = (*ref_sz).min(*cand_sz) as f64;
            if max_s / min_s.max(1.0) > 2.0 {
                continue;
            }
            let sim = sketch_similarity(ref_sk, cand_sk);
            if sim >= 0.35 && sim < 0.999 {
                pairs.insert(cand_rel.clone(), ref_rel.clone());
                used_as_ref.insert(ref_rel.clone(), true);
            }
        }
    }

    pairs
}

fn xor_residual_apply(target: &[u8], reference: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(target.len());
    let common = target.len().min(reference.len());
    for i in 0..common {
        out.push(target[i] ^ reference[i]);
    }
    if target.len() > common {
        out.extend_from_slice(&target[common..]);
    }
    out
}

#[allow(dead_code)]
pub struct UltraEncodeResult {
    pub compressed_size: u64,
    pub friendly_label: String,
    pub recipe: String,
}

/// Encodes `src` to `dest`. If `saved_recipe` is provided, skips transform search completely
/// for 5-8x faster re-compression!
pub fn encode_ultra_file(
    src: &Path,
    dest: &Path,
    reference_path: Option<&Path>,
    saved_recipe: Option<&str>,
    zstd_level: i32,
) -> Result<UltraEncodeResult> {
    let raw = fs::read(src)?;
    let is_dds = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("dds"))
        .unwrap_or(false);

    let mut is_delta = false;
    let working = if let Some(ref_p) = reference_path {
        if let Ok(ref_bytes) = fs::read(ref_p) {
            is_delta = true;
            xor_residual_apply(&raw, &ref_bytes)
        } else {
            raw.clone()
        }
    } else {
        raw.clone()
    };

    let ops = match saved_recipe {
        Some(r) if !r.trim().is_empty() => parse_recipe(r),
        _ => discover_best_pipeline(&working, is_dds && !is_delta),
    };

    let mut transformed = working;
    for &op in &ops {
        if op != TransformOp::None {
            transformed = apply_op_forward(&transformed, op);
        }
    }

    let payload = zstd::bulk::compress(&transformed, zstd_level)?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = File::create(dest)?;
    f.write_all(MAGIC)?;
    f.write_all(&[VERSION])?;
    f.write_all(&[if is_delta { 1 } else { 0 }])?;
    f.write_all(&[ops.len() as u8])?;
    for &op in &ops {
        f.write_all(&[op as u8])?;
    }
    f.write_all(&(raw.len() as u64).to_le_bytes())?;
    f.write_all(&payload)?;

    let compressed_size = fs::metadata(dest)
        .map(|m| m.len())
        .unwrap_or(payload.len() as u64);

    let used_bcn = ops
        .iter()
        .any(|o| matches!(o, TransformOp::BcnPlanar8 | TransformOp::BcnPlanar16));
    let used_smart = ops
        .iter()
        .any(|o| !matches!(o, TransformOp::None | TransformOp::BcnPlanar8 | TransformOp::BcnPlanar16));

    let friendly_label = if is_delta {
        "Ultra (Smart Delta)".to_string()
    } else if used_bcn {
        "Ultra (Texture Pack)".to_string()
    } else if used_smart {
        "Ultra (Smart Pack)".to_string()
    } else {
        "Ultra Pack".to_string()
    };

    Ok(UltraEncodeResult {
        compressed_size,
        friendly_label,
        recipe: serialize_recipe(&ops),
    })
}

/// Decodes a `.gsh` file back to its exact bit-for-bit original file.
pub fn decode_ultra_file(
    src: &Path,
    dest: &Path,
    reference_path: Option<&Path>,
) -> Result<u64> {
    let bytes = fs::read(src)?;
    if bytes.len() < 15 || &bytes[0..4] != MAGIC {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut inp = File::open(src)?;
        let mut out = File::create(dest)?;
        zstd::stream::copy_decode(&mut inp, &mut out)?;
        return Ok(fs::metadata(dest).map(|m| m.len()).unwrap_or(0));
    }

    let _ver = bytes[4];
    let is_delta = bytes[5] == 1;
    let op_count = bytes[6] as usize;
    let header_end = 7 + op_count + 8;
    if bytes.len() < header_end {
        return Err(anyhow!("Corrupt GSHK header in {}", src.display()));
    }

    let mut ops = Vec::with_capacity(op_count);
    for i in 0..op_count {
        ops.push(TransformOp::from_u8(bytes[7 + i])?);
    }

    let orig_len = u64::from_le_bytes(bytes[7 + op_count..header_end].try_into().unwrap()) as usize;
    let payload = &bytes[header_end..];

    let mut data = zstd::bulk::decompress(payload, orig_len + 1024)?;
    for &op in ops.iter().rev() {
        if op != TransformOp::None {
            data = apply_op_inverse(&data, op);
        }
    }

    if is_delta {
        let ref_p = reference_path.ok_or_else(|| {
            anyhow!("Missing Smart Delta reference file for {}", dest.display())
        })?;
        let ref_bytes = fs::read(ref_p)?;
        data = xor_residual_apply(&data, &ref_bytes);
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(dest, &data)?;
    Ok(data.len() as u64)
}
