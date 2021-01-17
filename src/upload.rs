#![cfg(not(target_arch = "wasm32"))]

use crate::{AssetIndex, AssetMetadata, Error};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use wrangler::{
    kv::bulk,
    settings::{global_user::GlobalUser, toml::Manifest},
    sites::{add_namespace, AssetManifest},
    terminal::message::{Message, StdErr},
};

const UPLOAD_PROGRESS_TEMPLATE: &str = "{wide_bar} {pos}/{len}\n{msg}";
const DELETE_PROGRESS_TEMPLATE: &str = "{wide_bar} {pos}/{len}\n{msg}";

pub struct SyncConfig<'sync> {
    /// Path to wrangler.toml. defaults to "wrangler.toml"
    pub wrangler_path: &'sync Path,
    /// Path to asset source folder, default: "public"
    pub asset_dir: &'sync Path,
    /// Path to output diretory: default: "data"
    pub output_path: &'sync Path,
    /// Remove stale files. Use this flag only after 'wrangler publish' completes. default: false
    pub prune: bool,
    /// True if using a preview environment. default=false
    pub preview_env: bool,
}

impl<'sync> Default for SyncConfig<'sync> {
    fn default() -> Self {
        Self {
            wrangler_path: Path::new("wrangler.toml"),
            asset_dir: Path::new("public"),
            output_path: Path::new("data"),
            prune: false,
            preview_env: false,
        }
    }
}

// Status of asset manifest
enum Update {
    New,
    NoChange,
    Updated,
}

/// Sync files
/// - scan the asset folder to determine which files need to be uploaded to KV storage;
/// - upload new files
/// - generate the manifest
/// - if the prune option is set, remove unreferenced files in the KV namespace
/// All the file system scanning and kv uploading is performed by wrangler library
pub fn sync_assets(args: SyncConfig) -> Result<(), Error> {
    // validate parameters
    match std::fs::metadata(&args.asset_dir) {
        Ok(md) if md.is_dir() => {}
        _ => {
            return Err(Error::InvalidAssetPath(
                args.asset_dir.to_string_lossy().to_string(),
            ))
        }
    }
    match std::fs::metadata(&args.wrangler_path) {
        Ok(md) if md.is_file() => {}
        _ => {
            return Err(Error::MissingWranglerFile(
                args.wrangler_path.to_string_lossy().to_string(),
            ))
        }
    }
    wrangler::commands::publish::validate_bucket_location(&PathBuf::from(args.asset_dir))?;

    // create parent of output dir
    mkdir_bin_parent(args.output_path)?;

    let manifest = Manifest::new(args.wrangler_path)?;
    let mut target = manifest.get_target(None, args.preview_env)?;
    let user = GlobalUser::new()?;

    let site_namespace = add_namespace(&user, &mut target, false)?;
    let (to_upload, to_delete, asset_manifest) =
        wrangler::sites::sync(&target, &user, &site_namespace.id, &args.asset_dir)?;

    let index = make_index(&args.asset_dir, asset_manifest)?;
    write_index(&args, index)?;

    // First, upload all existing files in asset_dir directory
    StdErr::working("Uploading site files");
    let upload_progress_bar = make_progress_bar(to_upload.len(), UPLOAD_PROGRESS_TEMPLATE);
    bulk::put(
        &target,
        &user,
        &site_namespace.id,
        to_upload,
        &upload_progress_bar,
    )?;

    if let Some(pb) = upload_progress_bar {
        pb.finish_with_message("Done Uploading");
    }

    // Finally, remove any stale files
    if !to_delete.is_empty() {
        if args.prune {
            StdErr::info("Pruning stale files...");
            let delete_progress_bar = make_progress_bar(to_delete.len(), DELETE_PROGRESS_TEMPLATE);
            bulk::delete(
                &target,
                &user,
                &site_namespace.id,
                to_delete,
                &delete_progress_bar,
            )?;

            if let Some(pb) = delete_progress_bar {
                pb.finish_with_message("Done deleting");
            }
        } else {
            StdErr::message(&format!(
                "Deferred pruning [{}] stale files. Run with '--prune' later to remove them.",
                to_delete.len()
            ));
        }
    }
    Ok(())
}

/// Generates the asset manifest
fn make_index(asset_dir: &Path, asset_manifest: AssetManifest) -> Result<AssetIndex, Error> {
    use std::time::SystemTime;

    let mut index: AssetIndex = AssetIndex::new();
    for (k, v) in asset_manifest.into_iter() {
        let asset_path = asset_dir.join(&k);
        let md = std::fs::metadata(&asset_path).map_err(|e| {
            Error::IO(format!(
                "failed reading asset file {}: {}",
                &asset_path.display(),
                e.to_string()
            ))
        })?;
        let modified = md
            .modified()
            .unwrap_or_else(|_| {
                panic!(
                    "Can't read modified time of {}. Please fix or run on a different platform",
                    &asset_path.display()
                )
            })
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_else(|_| panic!("Invalid timestamp for file {}", &asset_path.display()))
            .as_secs();
        index.insert(
            k,
            AssetMetadata {
                path: v,
                size: md.len(),
                modified,
            },
        );
    }
    Ok(index)
}

/// Serializes the asset manifest. Before writing it to a file, loads the previous file
/// to determine whether any changes are required. This lets us generate a friendlier and more
/// specific console message, and avoiding an unnecessary file write may shorten the next build time.
fn write_index(args: &SyncConfig, asset_index: AssetIndex) -> Result<(), Error> {
    let bytes = bincode::serialize(&asset_index)
        .map_err(|e| Error::IO(format!("serialization error: {}", e.to_string())))?;

    let update = match std::fs::read(args.output_path) {
        Ok(existing_bytes) => {
            if bytes.eq(&existing_bytes) {
                Update::NoChange
            } else {
                Update::Updated
            }
        }
        _ => Update::New,
    };
    match update {
        Update::New | Update::Updated => {
            std::fs::write(args.output_path, &bytes).map_err(|e| {
                Error::IO(format!(
                    "writing {}: {}",
                    args.output_path.display(),
                    e.to_string()
                ))
            })?;
        }
        _ => {}
    }
    StdErr::message(&format!(
        "{} asset manifest {}",
        match update {
            Update::New => "Generated",
            Update::Updated => "Updated",
            Update::NoChange => "No change to",
        },
        args.output_path.display()
    ));

    Ok(())
}

/// create the parent dir of the output file, if it doesn't exist already
fn mkdir_bin_parent(output_path: &Path) -> Result<(), Error> {
    if output_path.is_file() {
        return Ok(());
    }
    let parent = output_path
        .parent()
        .ok_or_else(|| Error::InvalidAssetsBinPath("Must not be in root dir".into()))?;

    if parent.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(parent).map_err(|e| {
        Error::IO(format!(
            "creating output directory {} for assets: {}",
            output_path.display(),
            e.to_string()
        ))
    })?;
    Ok(())
}

/// Draw progress bar on console
fn make_progress_bar(count: usize, template: &str) -> Option<ProgressBar> {
    if count > bulk::BATCH_KEY_MAX {
        let progress_bar = ProgressBar::new(count as u64);
        progress_bar.set_style(ProgressStyle::default_bar().template(template));
        Some(progress_bar)
    } else {
        None
    }
}
