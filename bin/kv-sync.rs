#![cfg(not(target_arch = "wasm32"))]

use clap::{Clap, ValueHint};
use kv_assets::{sync_assets, SyncConfig};
use std::path::PathBuf;

#[derive(Clap, Debug)]
struct Opt {
    /// Path to configuration file - defaults to "wrangler.toml"
    #[clap(short, long, parse(from_os_str), value_hint = ValueHint::FilePath, default_value="wrangler.toml")]
    wrangler: PathBuf,

    /// Path to assets dir - default to "public"
    #[clap(short,long, value_hint=ValueHint::DirPath, default_value="public")]
    assets: PathBuf,

    /// Path for generated asset index - defaults to "data/assets.bin"
    #[clap(short,long, value_hint=ValueHint::FilePath, default_value="data/assets.bin")]
    output: PathBuf,

    /// Dump contents of existing asset.bin file
    #[clap(long, value_hint=ValueHint::FilePath)]
    dump: Option<PathBuf>,

    /// Remove obsolete/unreferenced KV assets in the namespace. Use this flag only after successful publish
    #[clap(long)]
    prune: bool,
}

fn main() {
    let opt = Opt::parse();
    if let Err(e) = run(opt) {
        eprintln!("Error: {}", e.to_string());
        std::process::exit(2);
    }
}

fn run(opt: Opt) -> Result<(), kv_assets::Error> {
    if let Some(dump_file) = opt.dump {
        return dump(&dump_file);
    }
    let args = SyncConfig {
        output_path: &opt.output,
        wrangler_path: &opt.wrangler,
        asset_dir: &opt.assets,
        prune: opt.prune,
        ..Default::default()
    };
    sync_assets(args)?;
    Ok(())
}

fn dump(path: &std::path::Path) -> Result<(), kv_assets::Error> {
    use kv_assets::{AssetIndex, Error};

    let blob = std::fs::read(path).map_err(|e| {
        Error::Message(format!(
            "Error reading asset file {} for dump: {}",
            path.display(),
            e.to_string()
        ))
    })?;
    let map: AssetIndex = bincode::deserialize(&blob).map_err(Error::DeserializeAssets)?;
    let json = serde_json::to_string_pretty(&map)
        .map_err(|e| Error::Message(format!("json serialization error: {}", e.to_string())))?;
    println!("{}", json);
    Ok(())
}
