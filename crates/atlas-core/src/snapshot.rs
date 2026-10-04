//! Snapshot (de)serialisation (bincode): the stored parts of an [`Atlas`] and of the connected
//! [`Graph`]; the rest is derived on load. Two files, so a cache refresh rebuilds only the graph.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::atlas::Atlas;
use crate::disease::Disease;
use crate::error::CoreError;
use crate::graph::{Graph, GraphData};
use crate::identity::DiseaseIdentity;
use crate::provenance::Provenance;
use crate::term::Term;

/// Bump on any change to a serialised type.
pub const FORMAT: u32 = 2;
const MAGIC: &[u8; 8] = b"ATLASNAP";
/// Bump on any change to a serialised graph type.
pub const GRAPH_FORMAT: u32 = 7;
const GRAPH_MAGIC: &[u8; 8] = b"ATLGRAPH";

#[derive(Serialize)]
struct Out<'a> {
    signature: &'a str,
    hpo_terms: &'a [Term],
    identity: &'a DiseaseIdentity,
    provenance: &'a Provenance,
    diseases: &'a [Disease],
}

#[derive(Deserialize)]
struct In {
    signature: String,
    hpo_terms: Vec<Term>,
    identity: DiseaseIdentity,
    provenance: Provenance,
    diseases: Vec<Disease>,
}

/// Write `atlas` with an opaque `signature` of its inputs (the caller decides freshness).
/// Writes to a temporary file first, then renames.
pub fn save(path: &Path, atlas: &Atlas, signature: &str) -> Result<(), CoreError> {
    let (hpo_terms, identity, provenance, diseases) = atlas.parts();
    let out = Out {
        signature,
        hpo_terms,
        identity,
        provenance,
        diseases,
    };
    write(path, MAGIC, FORMAT, &out)
}

#[derive(Serialize)]
struct GraphOut<'a> {
    signature: &'a str,
    data: &'a GraphData,
}

#[derive(Deserialize)]
struct GraphIn {
    signature: String,
    data: GraphData,
}

/// Write the connected layer with the signature of its inputs.
pub fn save_graph(path: &Path, data: &GraphData, signature: &str) -> Result<(), CoreError> {
    write(path, GRAPH_MAGIC, GRAPH_FORMAT, &GraphOut { signature, data })
}

/// Signature of a graph snapshot (cheap freshness check).
pub fn graph_signature(path: &Path) -> Result<String, CoreError> {
    let mut r = open_as(path, GRAPH_MAGIC, GRAPH_FORMAT)?;
    bincode::deserialize_from(&mut r).map_err(|source| CoreError::Codec {
        path: path.to_owned(),
        source,
    })
}

/// Load the connected layer and build its indexes.
pub fn load_graph(path: &Path) -> Result<(Graph, String), CoreError> {
    let r = open_as(path, GRAPH_MAGIC, GRAPH_FORMAT)?;
    let data: GraphIn = bincode::deserialize_from(r).map_err(|source| CoreError::Codec {
        path: path.to_owned(),
        source,
    })?;
    Ok((Graph::new(data.data), data.signature))
}

fn write<T: Serialize>(path: &Path, magic: &[u8; 8], format: u32, value: &T) -> Result<(), CoreError> {
    let io = |source| CoreError::Io {
        path: path.to_owned(),
        source,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".part");
    let tmp = std::path::PathBuf::from(tmp);
    let mut w = BufWriter::new(File::create(&tmp).map_err(io)?);
    w.write_all(magic).map_err(io)?;
    w.write_all(&format.to_le_bytes()).map_err(io)?;
    bincode::serialize_into(&mut w, value).map_err(|source| CoreError::Codec {
        path: path.to_owned(),
        source,
    })?;
    w.flush().map_err(io)?;
    drop(w);
    std::fs::rename(&tmp, path).map_err(io)
}

/// Read the signature only (cheap freshness check).
pub fn signature(path: &Path) -> Result<String, CoreError> {
    let mut r = open(path)?;
    bincode::deserialize_from(&mut r).map_err(|source| CoreError::Codec {
        path: path.to_owned(),
        source,
    })
}

/// Load and rebuild derived state. Returns the atlas and its signature.
pub fn load(path: &Path) -> Result<(Atlas, String), CoreError> {
    let r = open(path)?;
    let data: In = bincode::deserialize_from(r).map_err(|source| CoreError::Codec {
        path: path.to_owned(),
        source,
    })?;
    let atlas = Atlas::new(data.hpo_terms, data.identity, data.provenance, data.diseases);
    Ok((atlas, data.signature))
}

fn open(path: &Path) -> Result<BufReader<File>, CoreError> {
    open_as(path, MAGIC, FORMAT)
}

fn open_as(path: &Path, magic: &[u8; 8], format: u32) -> Result<BufReader<File>, CoreError> {
    let io = |source| CoreError::Io {
        path: path.to_owned(),
        source,
    };
    let mut r = BufReader::with_capacity(1 << 20, File::open(path).map_err(io)?);
    let mut header = [0u8; 12];
    r.read_exact(&mut header).map_err(io)?;
    let found = u32::from_le_bytes(header[8..].try_into().unwrap());
    if &header[..8] != magic || found != format {
        return Err(CoreError::Format {
            path: path.to_owned(),
            found,
            expected: format,
        });
    }
    Ok(r)
}
