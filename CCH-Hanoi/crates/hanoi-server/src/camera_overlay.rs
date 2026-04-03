use std::fs::File;
use std::io;
use std::path::Path;

use arrow::array::{Array, Float32Array, UInt32Array};
use arrow::ipc::reader::FileReader;
use serde::Deserialize;

use crate::types::{CameraOverlayCamera, CameraOverlayQuery, CameraOverlayResponse};

#[derive(Clone)]
pub(crate) struct CameraMarker {
    id: Option<u64>,
    label: String,
    profile: Option<String>,
    arc_id: Option<u32>,
    lat: f32,
    lng: f32,
}

impl CameraMarker {
    fn intersects(&self, query: &CameraOverlayQuery) -> bool {
        self.lat >= query.min_lat
            && self.lat <= query.max_lat
            && self.lng >= query.min_lng
            && self.lng <= query.max_lng
    }

    fn to_response(&self) -> CameraOverlayCamera {
        CameraOverlayCamera {
            id: self.id,
            label: self.label.clone(),
            profile: self.profile.clone(),
            arc_id: self.arc_id,
            lat: self.lat,
            lng: self.lng,
        }
    }
}

pub(crate) enum CameraOverlay {
    Available { cameras: Vec<CameraMarker> },
    Unavailable { message: String },
}

impl CameraOverlay {
    pub fn load(manifest_path: &Path, camera_config_path: &Path) -> Self {
        match load_camera_markers(manifest_path, camera_config_path) {
            Ok(cameras) => CameraOverlay::Available { cameras },
            Err(error) => {
                tracing::warn!(
                    manifest = %manifest_path.display(),
                    camera_config = %camera_config_path.display(),
                    %error,
                    "camera overlay is unavailable"
                );
                CameraOverlay::Unavailable {
                    message: error.to_string(),
                }
            }
        }
    }

    pub fn render(&self, query: &CameraOverlayQuery) -> CameraOverlayResponse {
        match self {
            CameraOverlay::Available { cameras } => {
                let visible: Vec<CameraOverlayCamera> = cameras
                    .iter()
                    .filter(|camera| camera.intersects(query))
                    .map(CameraMarker::to_response)
                    .collect();

                CameraOverlayResponse {
                    available: true,
                    visible_camera_count: visible.len(),
                    total_camera_count: cameras.len(),
                    cameras: visible,
                    message: None,
                }
            }
            CameraOverlay::Unavailable { message } => CameraOverlayResponse {
                available: false,
                visible_camera_count: 0,
                total_camera_count: 0,
                cameras: Vec::new(),
                message: Some(message.clone()),
            },
        }
    }
}

#[derive(Deserialize)]
struct CameraConfigDocument {
    #[serde(default)]
    cameras: Vec<CameraConfigEntry>,
}

#[derive(Deserialize)]
struct CameraConfigEntry {
    id: Option<u64>,
    label: Option<String>,
    profile: Option<String>,
    arc_id: Option<u32>,
    lat: Option<f32>,
    lon: Option<f32>,
}

fn load_camera_markers(
    manifest_path: &Path,
    camera_config_path: &Path,
) -> io::Result<Vec<CameraMarker>> {
    let arc_midpoints = load_arc_midpoints(manifest_path)?;
    let yaml_text = std::fs::read_to_string(camera_config_path)?;
    let config: CameraConfigDocument =
        serde_yaml::from_str(&yaml_text).map_err(yaml_to_io_error)?;

    let mut cameras = Vec::with_capacity(config.cameras.len());
    for camera in config.cameras {
        if let Some(marker) = camera_to_marker(camera, &arc_midpoints) {
            cameras.push(marker);
        }
    }

    Ok(cameras)
}

fn camera_to_marker(
    camera: CameraConfigEntry,
    arc_midpoints: &[(f32, f32)],
) -> Option<CameraMarker> {
    let CameraConfigEntry {
        id,
        label,
        profile,
        arc_id,
        lat,
        lon,
    } = camera;

    let (lat, lng) = if let Some(arc_id) = arc_id {
        match arc_midpoints.get(arc_id as usize).copied() {
            Some(value) => value,
            None => {
                tracing::warn!(
                    arc_id,
                    "camera overlay entry references arc_id outside the manifest; skipping"
                );
                return None;
            }
        }
    } else if let (Some(lat), Some(lng)) = (lat, lon) {
        if !lat.is_finite() || !lng.is_finite() {
            tracing::warn!(
                camera_id = id,
                "camera overlay entry contains non-finite coordinates; skipping"
            );
            return None;
        }
        (lat, lng)
    } else {
        tracing::warn!(
            camera_id = id,
            "camera overlay entry is missing arc_id and lat/lon; skipping"
        );
        return None;
    };

    let label = label
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            id.map(|id| format!("Camera {}", id))
                .unwrap_or_else(|| "Camera".to_string())
        });

    Some(CameraMarker {
        id,
        label,
        profile,
        arc_id,
        lat,
        lng,
    })
}

fn load_arc_midpoints(manifest_path: &Path) -> io::Result<Vec<(f32, f32)>> {
    let file = File::open(manifest_path)?;
    let reader = FileReader::try_new(file, None).map_err(arrow_to_io_error)?;

    let mut arc_midpoints = Vec::<(f32, f32)>::new();

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
        let tail_lat = batch
            .column_by_name("tail_lat")
            .and_then(|column| column.as_any().downcast_ref::<Float32Array>())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "road_arc_manifest.arrow is missing a float32 'tail_lat' column",
                )
            })?;
        let tail_lon = batch
            .column_by_name("tail_lon")
            .and_then(|column| column.as_any().downcast_ref::<Float32Array>())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "road_arc_manifest.arrow is missing a float32 'tail_lon' column",
                )
            })?;
        let head_lat = batch
            .column_by_name("head_lat")
            .and_then(|column| column.as_any().downcast_ref::<Float32Array>())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "road_arc_manifest.arrow is missing a float32 'head_lat' column",
                )
            })?;
        let head_lon = batch
            .column_by_name("head_lon")
            .and_then(|column| column.as_any().downcast_ref::<Float32Array>())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "road_arc_manifest.arrow is missing a float32 'head_lon' column",
                )
            })?;

        for row in 0..batch.num_rows() {
            if arc_ids.is_null(row)
                || tail_lat.is_null(row)
                || tail_lon.is_null(row)
                || head_lat.is_null(row)
                || head_lon.is_null(row)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "road_arc_manifest.arrow row {} contains null geometry fields",
                        row
                    ),
                ));
            }

            let arc_id = arc_ids.value(row) as usize;
            if arc_id >= arc_midpoints.len() {
                arc_midpoints.resize(arc_id + 1, (f32::NAN, f32::NAN));
            }
            if arc_midpoints[arc_id].0.is_finite() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "road_arc_manifest.arrow contains duplicate arc_id {}",
                        arc_id
                    ),
                ));
            }

            arc_midpoints[arc_id] = (
                (tail_lat.value(row) + head_lat.value(row)) / 2.0,
                (tail_lon.value(row) + head_lon.value(row)) / 2.0,
            );
        }
    }

    if let Some(missing_arc_id) = arc_midpoints
        .iter()
        .position(|(lat, lng)| !lat.is_finite() || !lng.is_finite())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "road_arc_manifest.arrow is missing arc_id {}",
                missing_arc_id
            ),
        ));
    }

    Ok(arc_midpoints)
}

fn arrow_to_io_error(error: arrow::error::ArrowError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn yaml_to_io_error(error: serde_yaml::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
