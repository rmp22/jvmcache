use crate::constants::{DIR_OBJECTS, FILE_MANIFEST};
use crate::domain::{CompilerKind, Manifest};
use crate::telemetry::TelemetryLogger;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ManifestSummary {
    pub key: String,
    pub compiler: CompilerKind,
    pub created_at: String,
    pub artifact_count: usize,
    pub total_bytes: u64,
    pub sample_artifacts: Vec<String>,
}

pub struct ObjectInspector;

impl ObjectInspector {
    pub fn list_objects(
        cache_dir: &Path,
        filter: Option<&str>,
        limit: usize,
    ) -> Vec<ManifestSummary> {
        let objects_dir = cache_dir.join(DIR_OBJECTS);
        if !objects_dir.exists() {
            return Vec::new();
        }

        let mut manifests = Vec::new();
        let walker = walkdir::WalkDir::new(&objects_dir)
            .min_depth(2)
            .max_depth(3)
            .into_iter();

        for entry in walker.flatten() {
            if entry.file_type().is_file() && entry.file_name() == FILE_MANIFEST {
                if let Ok(content) = fs::read_to_string(entry.path()) {
                    if let Ok(manifest) = serde_json::from_str::<Manifest>(&content) {
                        if let Some(pat) = filter {
                            let matches_key = manifest.cache_key.contains(pat);
                            let matches_compiler =
                                format!("{:?}", manifest.compiler_kind).contains(pat);
                            let matches_artifact = manifest
                                .artifacts
                                .iter()
                                .any(|a| a.rel_path.to_string_lossy().contains(pat));
                            if !matches_key && !matches_compiler && !matches_artifact {
                                continue;
                            }
                        }

                        let total_bytes: u64 =
                            manifest.artifacts.iter().map(|a| a.size_bytes).sum();
                        let sample: Vec<String> = manifest
                            .artifacts
                            .iter()
                            .take(4)
                            .map(|a| a.rel_path.to_string_lossy().into_owned())
                            .collect();

                        manifests.push(ManifestSummary {
                            key: manifest.cache_key,
                            compiler: manifest.compiler_kind,
                            created_at: TelemetryLogger::format_epoch_secs(
                                manifest.created_at_epoch_secs,
                            ),
                            artifact_count: manifest.artifacts.len(),
                            total_bytes,
                            sample_artifacts: sample,
                        });
                    }
                }
            }
        }

        manifests.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        manifests.truncate(limit);
        manifests
    }
}
