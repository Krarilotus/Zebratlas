//! HGNC complete set: approved genes with aliases, previous symbols and NCBI Gene ids.

use std::path::Path;

use atlas_core::provenance::{EntityIdx, RecordRef};

use crate::error::IngestError;
use crate::mechanism::{column, line_of, tsv_reader};

pub use atlas_core::mechanism::hgnc::{Hgnc, HgncGene};

pub fn read(path: &Path, entity: EntityIdx) -> Result<Hgnc, IngestError> {
    let mut rdr = tsv_reader(path)?;
    let headers = rdr
        .headers()
        .map_err(|source| IngestError::Csv {
            path: path.to_owned(),
            source,
        })?
        .clone();
    let col = |name: &'static str| column(&headers, name, path);
    let (hgnc_id, symbol, name, locus_group, status) = (
        col("hgnc_id")?,
        col("symbol")?,
        col("name")?,
        col("locus_group")?,
        col("status")?,
    );
    let (alias, prev, group, entrez) = (
        col("alias_symbol")?,
        col("prev_symbol")?,
        col("gene_group")?,
        col("entrez_id")?,
    );
    let mut out = Hgnc::default();
    let mut rec = csv::StringRecord::new();
    while rdr.read_record(&mut rec).map_err(|source| IngestError::Csv {
        path: path.to_owned(),
        source,
    })? {
        let line = line_of(&rec);
        if &rec[status] != "Approved" {
            out.skipped_not_approved += 1;
            continue;
        }
        let split = |i: usize| -> Vec<String> {
            rec[i]
                .trim_matches('"')
                .split('|')
                .filter(|x| !x.is_empty())
                .map(str::to_owned)
                .collect()
        };
        out.genes.push(HgncGene {
            hgnc_id: rec[hgnc_id].to_owned(),
            symbol: rec[symbol].to_owned(),
            name: rec[name].to_owned(),
            locus_group: rec[locus_group].to_owned(),
            entrez: Some(rec[entrez].to_owned()).filter(|s| !s.is_empty()),
            groups: split(group),
            aliases: split(alias),
            previous: split(prev),
            record: RecordRef::line(entity, line),
        });
    }
    out.finish();
    Ok(out)
}
