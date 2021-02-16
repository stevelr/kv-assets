use crate::Error;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;

const CLOUDFLARE_KV_ENDPOINT: &str = "https://api.cloudflare.com/client/v4";

/// Hashmap of asset paths to metadata
/// Path strings have leading / removed
pub type AssetIndex = std::collections::HashMap<String, AssetMetadata>;

/// Asset metadata
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub struct AssetMetadata {
    /// Path to file within the namespace
    pub path: String,
    /// Last modified time of file, in UTC seconds since EPOCH
    pub modified: u64,
    /// Size of file
    pub size: u64,
}

/// Serves static assets out of Worker KV storage.
#[allow(clippy::upper_case_acronyms)]
pub struct KVAssets<'ah> {
    index: &'ah [u8],
    map: RefCell<Option<AssetIndex>>,
    kv: KV,
}

/// Workers KV Parameters
#[allow(clippy::upper_case_acronyms)]
pub struct KV {
    account_id: String,
    namespace_id: String,
    auth_token: String,
}

/// Initialize KV parameters
pub fn init_kv<T: ToString>(account: T, namespace: T, token: T) -> KV {
    KV {
        account_id: account.to_string(),
        namespace_id: namespace.to_string(),
        auth_token: token.to_string(),
    }
}

impl<'ah> KVAssets<'ah> {
    /// Initialize handler
    /// - index: binary serialized index (created by cf_assets)
    /// - account_id: cloudflare account id
    /// - namespace_id: cloudflare namespace (printed by cf_assets)
    /// - auth_token: cloudflare OAuth token
    pub fn init(
        index: &'ah [u8],
        account_id: &'_ str,
        namespace_id: &'_ str,
        auth_token: &'_ str,
    ) -> Self {
        Self {
            index,
            map: RefCell::new(None),
            kv: init_kv(account_id, namespace_id, auth_token),
        }
    }

    /// Initialize with exiting KV parameters
    pub fn init_with(index: &'ah [u8], kv: KV) -> Self {
        Self {
            index,
            map: RefCell::new(None),
            kv,
        }
    }

    // Lazily deserialize map, so we don't bother doing so
    // when handling urls that aren't for static assets
    fn ensure_map(&self) -> Result<(), Error> {
        let mut map = self.map.borrow_mut();
        if (*map).is_none() {
            *map = Some(
                bincode::deserialize(self.index)
                    .map_err(|e| Error::DeserializeAssets(e.to_string()))?,
            );
        }
        Ok(())
    }

    /// all-in-one method to get the asset from KV
    pub async fn get_asset(&self, key: &str) -> Result<Option<bytes::Bytes>, Error> {
        match self.lookup_key(key) {
            Ok(Some(md)) => {
                let doc = self.kv.get_kv_value(&md.path).await?;
                Ok(Some(doc))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Finds the path in the map, returning the "key"
    /// This lookup should reliably and quickly determine whether asset is in KV,
    /// as it doesn't require querying KV yet.
    /// Removes leading / if present
    /// Returns Ok(None) if Not found
    pub fn lookup_key(&self, path: &'_ str) -> Result<Option<AssetMetadata>, Error> {
        // remove leading '/' if present
        let path = path.strip_prefix('/').unwrap_or(path);
        if path.is_empty() {
            return Err(Error::EmptyKey);
        }
        self.ensure_map()?;
        let map = self.map.borrow();
        let md = map.as_ref().unwrap().get(path).cloned();
        Ok(md)
    }

    /// Lookup asset in worker kV storage.
    /// If the key passed had been obtained from lookup_key, but the value was not found,
    /// then one of the following occurred:
    /// - the asset was deleted from KV
    /// - the value timed out via TTL
    /// - the index is out of date
    pub async fn get_kv_value(&self, key: &str) -> Result<bytes::Bytes, Error> {
        self.kv.get_kv_value(key).await
    }

    /// Store a value in KV. Optionally, set expiration TTL, number of seconds in future
    /// when content should be automatically deleted. TTL must be at least 60.
    pub async fn put_kv_value<T: Into<reqwest::Body>>(
        &self,
        key: &str,
        val: T,
        expiration_ttl: Option<u64>,
    ) -> Result<(), Error> {
        self.kv.put_kv_value(key, val, expiration_ttl).await
    }
}

impl KV {
    /// Lookup asset in worker kV storage.
    /// If the key passed had been obtained from lookup_key, but the value was not found,
    /// then one of the following occurred:
    /// - the asset was deleted from KV
    /// - the value timed out via TTL
    /// - the index is out of date
    pub async fn get_kv_value(&self, key: &str) -> Result<bytes::Bytes, Error> {
        let url = format!(
            "{}/accounts/{}/storage/kv/namespaces/{}/values/{}",
            CLOUDFLARE_KV_ENDPOINT, &self.account_id, &self.namespace_id, key
        );
        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .send()
            .await
            .map_err(|e| Error::KVHttp(e.to_string(), String::new()))?;
        match response.status().is_success() {
            false => Err(Error::KVKeyNotFound(
                key.to_string(),
                response.status().as_u16(),
            )),
            true => Ok(response
                .bytes()
                .await
                .map_err(|e| Error::KVHttp(e.to_string(), String::new()))?),
        }
    }

    /// Delete the key at path.
    pub async fn delete_kv_value(&self, key: &str) -> Result<(), Error> {
        let url = format!(
            "{}/accounts/{}/storage/kv/namespaces/{}/values/{}",
            CLOUDFLARE_KV_ENDPOINT, &self.account_id, &self.namespace_id, key,
        );
        let client = reqwest::Client::new();
        let resp = client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .send()
            .await
            .map_err(|e| Error::KVHttp(e.to_string(), String::new()))?;
        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| Error::KVHttp(e.to_string(), String::new()))?;
        if !status.is_success() {
            return Err(Error::KVHttpStatus(
                status.as_u16(),
                String::from_utf8_lossy(&bytes).to_string(),
            ));
        }
        Ok(())
    }

    /// Store a value in KV. Optionally, set expiration TTL, number of seconds in future
    /// when content should be automatically deleted. TTL must be at least 60.
    pub async fn put_kv_value<T: Into<reqwest::Body>>(
        &self,
        key: &str,
        val: T,
        expiration_ttl: Option<u64>,
    ) -> Result<(), Error> {
        let url = format!(
            "{}/accounts/{}/storage/kv/namespaces/{}/values/{}{}",
            CLOUDFLARE_KV_ENDPOINT,
            &self.account_id,
            &self.namespace_id,
            key,
            match expiration_ttl {
                Some(ttl) => {
                    if ttl < 60 {
                        return Err(Error::TTLTooShort);
                    }
                    format!("?expiration_ttl={}", ttl)
                }
                None => String::from(""),
            }
        );

        let client = reqwest::Client::new();
        let resp = client
            .put(&url)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .body(val)
            .send()
            .await
            .map_err(|e| Error::KVHttp(e.to_string(), String::new()))?;
        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| Error::KVHttp(e.to_string(), String::new()))?;
        if !status.is_success() {
            return Err(Error::KVHttpStatus(
                status.as_u16(),
                String::from_utf8_lossy(&bytes).to_string(),
            ));
        }
        let resp: WriteKVResponse = match serde_json::from_slice(&bytes) {
            Ok(wr) => Ok(wr),
            Err(e) => Err(Error::KVHttp(
                e.to_string(),
                String::from_utf8_lossy(&bytes).to_string(),
            )),
        }?;
        if resp.success {
            Ok(())
        } else {
            Err(Error::Message(format!(
                "writing key {}: errors:{:?} messages:{:?}",
                key, resp.errors, resp.messages
            )))
        }
    }
}

#[derive(Debug, Deserialize)]
#[allow(clippy::upper_case_acronyms)]
struct WriteKVResponse {
    success: bool,
    errors: Vec<serde_json::Value>,
    messages: Vec<serde_json::Value>,
}

/// Tests manifest lookup function (does not invoke cloudflare api)
#[test]
fn test_lookup() {
    let md_ab = AssetMetadata {
        path: "a/b.txt".to_string(),
        modified: 10000,
        size: 10,
    };
    let md_b = AssetMetadata {
        path: "b".to_string(),
        modified: 20000,
        size: 20,
    };
    let md_c = AssetMetadata {
        path: "c.json".to_string(),
        modified: 30000,
        size: 30,
    };
    let mut index = AssetIndex::new();
    index.insert("a/b".to_string(), md_ab.clone());
    index.insert("b".to_string(), md_b.clone());
    index.insert("c.json".to_string(), md_c.clone());
    let blob = bincode::serialize(&index).expect("serialize-index");

    let kv = KVAssets::init(&blob, "123", "namespace", "token");

    // lookup
    assert_eq!(kv.lookup_key("a/b").unwrap().as_ref(), Some(&md_ab));
    assert_eq!(kv.lookup_key("b").unwrap().as_ref(), Some(&md_b));
    assert_eq!(kv.lookup_key("c.json").unwrap().as_ref(), Some(&md_c));

    // lookup not found
    assert_eq!(kv.lookup_key("xyz").unwrap(), None);

    // test strip prefix
    assert_eq!(kv.lookup_key("/b").unwrap().as_ref(), Some(&md_b));

    // ensure_map
    assert!(kv.ensure_map().is_ok());
}
