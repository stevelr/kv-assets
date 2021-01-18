# kv-assets & kv-sync

Store static assets in Workers KV for Rust-WASM Worker HTTP servers

The command-line program `kv-sync`, is a build tool that syncs
files between a local folder and Workers KV,
and generates a small manifest that is published with the worker wasm.

This crate was originally developed to support `StaticFileHandler` from
[`wasm-service`](https://github.com/stevelr/wasm-service),
to add static file support for Rust Worker-based http servers.

Although `wrangler` does a similar kind of sync for projects 
configured as a Workers "Site", 
(and we do reuse a lot of code from `wrangler` in this implementation),
there are a few differences:

- this works with Rust projects
  
- The generated manifest is bincode-serialized to a local file, and
  compiled into the wasm package as a static byte array using,
  for example, `include_bytes!("../data/assets.bin")`.
  
- Whereas a wrangler site manifest contains just the KV key path,
  this manifest contains additional metadata such as last modified time
  (from the file system), which can be used to process the HTTP header
  "If-Modified-Since", so the worker can decide
  whether to return 304-Not-Modified without needing to access KV.
  

## Installation

To install kv-sync, `cargo install kv-assets`, and the program `kv-sync`
will be added to `.cargo/bin`. 

If you want to include the kv-assets library in your project, add a line
to Cargo.toml:

    `kv-assets = "0.2"`


## `kv-sync` operations

`kv-sync` does the following:

- Generates `AssetIndex` of all files in the specified directory,
  (default `./public`), skipping files beginning with "." 
  or mentioned in .gitignore or .ignore
  (the search path for .gitignore and .ignore includes the asset folder
  and its parent)
  `AssetIndex` is a `HashMap<String,AssetMetadata>`, where
  the key is the relative path from the top asset folder.
  The `AssetIndex` is serialized with 
  [`bincode`](https://crates.io/crates/bincode) into a local file.
  
- Uploads new and updated files to KV storage, using a KV key
  that includes a file checksum to act as a unique version id.
  
- If you run `kv-sync --prune`, it will prune KV storage
  by removing obsolete files (previous versions no longer referenced).
  Don't use this flag until the code (with the updated assets.bin) 
  has been successively published, though, or else your'll get file not found errors.
  
  
## Adding `kv-sync` to dev workflow

Run `kv-sync` at least once before publishing the worker the first time.
This will upload the files _and_ generate the manifest.

It is not necessary to run `kv-sync` again until there is a change
to assets (anything in the `public` folder). 
If a file in that folder changes, run the following:

```sh
kv-sync
wrangler publish
# if wrangler publish succeeded without errors, then also run
kv-sync --prune
```

The first kv-sync regnerates the manifest and uploads modified files;
and wrangler publish publishes the code with the updated manifest.
if publish succeeds, it is safe
to run the prune step to remove old assets in KV storage.
    
