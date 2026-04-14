use std::{
    fmt::Write,
    fs,
    io::{self, Error, ErrorKind},
    path::{Path, PathBuf},
};

use chrono::{SecondsFormat, Utc};
use rust_road_router::{
    algo::customizable_contraction_hierarchy::{
        CCH, CCHReconstrctor, DirectedCCH, DirectedCCHReconstructor,
    },
    datastr::graph::EdgeIdGraph,
    io::{Deconstruct, ReconstructPrepared},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const CACHE_VERSION: u32 = 1;
const CACHE_META_FILE: &str = "cache_meta";

#[derive(Debug, Serialize, Deserialize)]
struct CacheMeta {
    version: u32,
    endianness: String,
    pointer_width: usize,
    source_checksum: String,
    created_utc: String,
}

fn current_endianness() -> &'static str {
    if cfg!(target_endian = "little") {
        "little"
    } else {
        "big"
    }
}

pub struct CchCache {
    cache_dir: PathBuf,
}

impl CchCache {
    pub fn new(graph_dir: &Path) -> Self {
        Self {
            cache_dir: graph_dir.join("cch_cache"),
        }
    }

    pub fn is_valid(&self, source_files: &[&Path]) -> bool {
        let meta = match self.read_meta() {
            Ok(meta) => meta,
            Err(err) => {
                tracing::debug!("CCH cache metadata unavailable: {err}");
                return false;
            }
        };

        if meta.version != CACHE_VERSION {
            tracing::debug!(
                cache_version = meta.version,
                expected = CACHE_VERSION,
                "CCH cache version mismatch"
            );
            return false;
        }
        if meta.endianness != current_endianness() {
            tracing::debug!(
                cache_endianness = %meta.endianness,
                expected = current_endianness(),
                "CCH cache endianness mismatch"
            );
            return false;
        }
        if meta.pointer_width != size_of::<usize>() {
            tracing::debug!(
                cache_pointer_width = meta.pointer_width,
                expected = size_of::<usize>(),
                "CCH cache pointer width mismatch"
            );
            return false;
        }

        let checksum = match Self::compute_checksum(source_files) {
            Ok(checksum) => checksum,
            Err(err) => {
                tracing::warn!("failed to compute CCH source checksum: {err}");
                return false;
            }
        };

        if meta.source_checksum != checksum {
            tracing::debug!("CCH cache checksum mismatch");
            return false;
        }

        true
    }

    pub fn save(&self, cch: &DirectedCCH, source_files: &[&Path]) -> io::Result<()> {
        self.save_impl(cch, source_files)
    }

    pub fn load(&self) -> io::Result<DirectedCCH> {
        DirectedCCHReconstructor.reconstruct_from(&self.cache_dir)
    }

    pub fn save_cch(&self, cch: &CCH, source_files: &[&Path]) -> io::Result<()> {
        self.save_impl(cch, source_files)
    }

    pub fn load_cch<Graph: EdgeIdGraph>(&self, graph: &Graph) -> io::Result<CCH> {
        CCHReconstrctor(graph).reconstruct_from(&self.cache_dir)
    }

    fn save_impl<T: Deconstruct>(&self, value: &T, source_files: &[&Path]) -> io::Result<()> {
        fs::create_dir_all(&self.cache_dir)?;
        value.deconstruct_to(&self.cache_dir)?;
        self.write_meta(source_files)
    }

    fn meta_path(&self) -> PathBuf {
        self.cache_dir.join(CACHE_META_FILE)
    }

    fn read_meta(&self) -> io::Result<CacheMeta> {
        let bytes = fs::read(self.meta_path())?;
        serde_json::from_slice(&bytes).map_err(|err| {
            Error::new(
                ErrorKind::InvalidData,
                format!("invalid cache_meta JSON: {err}"),
            )
        })
    }

    fn write_meta(&self, source_files: &[&Path]) -> io::Result<()> {
        let meta = CacheMeta {
            version: CACHE_VERSION,
            endianness: current_endianness().to_string(),
            pointer_width: size_of::<usize>(),
            source_checksum: Self::compute_checksum(source_files)?,
            created_utc: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        };
        let bytes = serde_json::to_vec_pretty(&meta).map_err(|err| {
            Error::new(
                ErrorKind::Other,
                format!("failed to serialize cache_meta: {err}"),
            )
        })?;
        fs::write(self.meta_path(), bytes)
    }

    fn compute_checksum(source_files: &[&Path]) -> io::Result<String> {
        let mut hasher = Sha256::new();
        for path in source_files {
            let bytes = fs::read(path)?;
            hasher.update(&bytes);
        }
        let digest = hasher.finalize();
        let mut checksum = String::with_capacity(digest.len() * 2);
        for byte in digest {
            write!(&mut checksum, "{byte:02x}").map_err(|err| {
                Error::new(
                    ErrorKind::Other,
                    format!("failed to encode checksum: {err}"),
                )
            })?;
        }
        Ok(checksum)
    }
}
