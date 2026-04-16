use std::path::Path;

use super::loader::load_camera_markers;
use crate::api::dto::{CameraOverlayCamera, CameraOverlayQuery, CameraOverlayResponse};

#[derive(Clone)]
pub(crate) struct CameraMarker {
    pub(super) id: Option<u64>,
    pub(super) label: String,
    pub(super) profile: Option<String>,
    pub(super) arc_id: Option<u32>,
    pub(super) lat: f32,
    pub(super) lng: f32,
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
