use std::fs::File;
use std::io;
use std::path::Path;

use arrow::array::{Array, StringArray, UInt32Array};
use arrow::ipc::reader::FileReader;

pub(crate) fn load_major_road_flags_or_default(
    manifest_path: &Path,
    expected_arc_count: usize,
) -> (Vec<bool>, bool) {
    match load_major_road_flags(manifest_path, expected_arc_count) {
        Ok(flags) => (flags, true),
        Err(error) => {
            tracing::warn!(
                manifest = %manifest_path.display(),
                %error,
                "traffic overlay road-class filter is unavailable; falling back to unfiltered support"
            );
            (vec![true; expected_arc_count], false)
        }
    }
}

fn load_major_road_flags(manifest_path: &Path, expected_arc_count: usize) -> io::Result<Vec<bool>> {
    let file = File::open(manifest_path)?;
    let reader = FileReader::try_new(file, None).map_err(arrow_to_io_error)?;

    let mut flags = vec![false; expected_arc_count];
    let mut seen = vec![false; expected_arc_count];

    for maybe_batch in reader {
        let batch = maybe_batch.map_err(arrow_to_io_error)?;
        let arc_ids = batch
            .column_by_name("arc_id")
            .and_then(|column| column.as_any().downcast_ref::<UInt32Array>())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "road_arc_manifest.arrow is missing a uint32 'arc_id' column",
                )
            })?;
        let highways = batch
            .column_by_name("highway")
            .and_then(|column| column.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "road_arc_manifest.arrow is missing a string 'highway' column",
                )
            })?;

        for row in 0..batch.num_rows() {
            if arc_ids.is_null(row) || highways.is_null(row) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "road_arc_manifest.arrow row {} contains null arc_id/highway",
                        row
                    ),
                ));
            }

            let arc_id = arc_ids.value(row) as usize;
            if arc_id >= expected_arc_count {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "road_arc_manifest.arrow contains arc_id {} outside expected range 0..{}",
                        arc_id,
                        expected_arc_count.saturating_sub(1)
                    ),
                ));
            }
            if seen[arc_id] {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "road_arc_manifest.arrow contains duplicate arc_id {}",
                        arc_id
                    ),
                ));
            }

            seen[arc_id] = true;
            flags[arc_id] = is_tertiary_or_above(highways.value(row));
        }
    }

    if let Some(missing_arc_id) = seen.iter().position(|present| !present) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "road_arc_manifest.arrow is missing arc_id {}",
                missing_arc_id
            ),
        ));
    }

    Ok(flags)
}

fn arrow_to_io_error(error: arrow::error::ArrowError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn is_tertiary_or_above(highway: &str) -> bool {
    let normalized = highway.strip_suffix("_link").unwrap_or(highway);
    matches!(
        normalized,
        "motorway" | "motorway_junction" | "trunk" | "primary" | "secondary" | "tertiary"
    )
}
