//! Obsidian plugins in the vault, for the apps' plugin page and the plugin
//! runtime: what is installed, what is turned on, each one's code, its
//! saved settings, and the web requests plugins make (`requestUrl`), which
//! the apps let through only to the sites the person allowed.

use std::time::Duration;

use paperloom_http::{Http, Request, UreqHttp};
use serde::{Deserialize, Serialize};

use crate::{CoreError, PaperVault};

/// A plugin installed in the vault (`.obsidian/plugins/<id>/`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct ObsidianPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub min_app_version: String,
    pub description: String,
    pub author: String,
    pub author_url: String,
    /// It needs Node or Electron, so it stays off on phones.
    pub is_desktop_only: bool,
    /// Turned on (listed in `community-plugins.json`).
    pub enabled: bool,
    pub has_styles: bool,
}

/// What it takes to run a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct ObsidianPluginCode {
    pub main: String,
    pub styles: Option<String>,
    /// Its settings as it saved them (`data.json`).
    pub data: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct PluginHeader {
    pub name: String,
    pub value: String,
}

/// A plugin request's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct PluginResponse {
    pub status: u16,
    pub text: String,
}

/// A plugin in Obsidian's community list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct CommunityPlugin {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// `owner/name` on GitHub, where its releases are.
    pub repo: String,
}

/// Obsidian's list of community plugins.
const CATALOG: &str =
    "https://raw.githubusercontent.com/obsidianmd/obsidian-releases/master/community-plugins.json";

fn network(e: paperloom_http::NetworkError) -> CoreError {
    CoreError::Network { message: e.0 }
}

/// The community plugins Obsidian lists, to browse and install.
#[uniffi::export]
pub fn community_plugins() -> Result<Vec<CommunityPlugin>, CoreError> {
    catalog_with(&UreqHttp::new())
}

fn catalog_with(http: &dyn Http) -> Result<Vec<CommunityPlugin>, CoreError> {
    let response = http.get(CATALOG, "application/json").map_err(network)?;
    if !response.is_success() {
        return Err(CoreError::Network {
            message: format!("The plugin list could not be fetched ({})", response.status),
        });
    }
    serde_json::from_str(&response.body)
        .map_err(|e| CoreError::InvalidInput { message: format!("The plugin list could not be read: {e}") })
}

/// The methods a plugin may use.
fn method(name: &str) -> Option<&'static str> {
    ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"].into_iter().find(|m| m.eq_ignore_ascii_case(name))
}

/// Makes a web request for a plugin (`requestUrl`). Only `http` and
/// `https`; the app has asked the person about the site first.
#[uniffi::export]
pub fn plugin_fetch(
    url: String,
    method: String,
    headers: Vec<PluginHeader>,
    body: Option<String>,
) -> Result<PluginResponse, CoreError> {
    fetch_with(&UreqHttp::with_timeout(Duration::from_secs(60)), url, &method, headers, body)
}

fn fetch_with(
    http: &dyn Http,
    url: String,
    method_name: &str,
    headers: Vec<PluginHeader>,
    body: Option<String>,
) -> Result<PluginResponse, CoreError> {
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("https://") && !lower.starts_with("http://") {
        return Err(CoreError::InvalidInput {
            message: format!("Plugins may only reach web addresses, not {url}"),
        });
    }
    let method = method(method_name).ok_or_else(|| CoreError::InvalidInput {
        message: format!("Plugins may not send {method_name} requests"),
    })?;
    let request = Request {
        method,
        url,
        headers: headers.into_iter().map(|h| (h.name, h.value)).collect(),
        body: body.map(String::into_bytes),
    };
    let answer = http.send(request).map_err(|e| CoreError::Network { message: e.0 })?;
    Ok(PluginResponse { status: answer.status, text: String::from_utf8_lossy(&answer.body).into_owned() })
}

#[uniffi::export]
impl PaperVault {
    /// The Obsidian plugins in the vault, by name.
    pub fn obsidian_plugins(&self) -> Result<Vec<ObsidianPlugin>, CoreError> {
        Ok(self
            .lock()?
            .obsidian_plugins()
            .into_iter()
            .map(|p| ObsidianPlugin {
                id: p.manifest.id,
                name: p.manifest.name,
                version: p.manifest.version,
                min_app_version: p.manifest.min_app_version,
                description: p.manifest.description,
                author: p.manifest.author,
                author_url: p.manifest.author_url,
                is_desktop_only: p.manifest.is_desktop_only,
                enabled: p.enabled,
                has_styles: p.has_styles,
            })
            .collect())
    }

    /// Turns a plugin on or off, for Obsidian too.
    pub fn set_obsidian_plugin_enabled(&self, id: String, enabled: bool) -> Result<(), CoreError> {
        Ok(self.lock()?.set_obsidian_plugin_enabled(&id, enabled)?)
    }

    pub fn obsidian_plugin_code(&self, id: String) -> Result<ObsidianPluginCode, CoreError> {
        let code = self.lock()?.obsidian_plugin_code(&id)?;
        Ok(ObsidianPluginCode { main: code.main, styles: code.styles, data: code.data })
    }

    /// Downloads a community plugin's latest release from its GitHub
    /// repository (`owner/name`) into the vault, as Obsidian does; it stays
    /// off until turned on.
    pub fn install_obsidian_plugin(&self, repo: String) -> Result<ObsidianPlugin, CoreError> {
        self.install_with(&UreqHttp::with_timeout(Duration::from_secs(120)), &repo)
    }

    /// Every note's metadata as Obsidian's `metadataCache` gives it to
    /// plugins, as JSON (by path).
    pub fn obsidian_metadata(&self) -> Result<String, CoreError> {
        Ok(self.lock()?.obsidian_metadata().to_string())
    }

    /// Keeps a plugin's settings (JSON), as its `saveData` asks.
    pub fn save_obsidian_plugin_data(&self, id: String, data: String) -> Result<(), CoreError> {
        Ok(self.lock()?.save_obsidian_plugin_data(&id, &data)?)
    }
}

impl PaperVault {
    fn install_with(&self, http: &dyn Http, repo: &str) -> Result<ObsidianPlugin, CoreError> {
        let parts: Vec<&str> = repo.split('/').collect();
        let plain = |part: &&str| {
            !part.is_empty()
                && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        };
        if parts.len() != 2 || !parts.iter().all(plain) || parts.iter().any(|p| p.starts_with('.')) {
            return Err(CoreError::InvalidInput { message: format!("{repo} is not a GitHub repository") });
        }
        let release = |name: &str| -> Result<Option<String>, CoreError> {
            let url = format!("https://github.com/{repo}/releases/latest/download/{name}");
            let response = http.get(&url, "*/*").map_err(network)?;
            Ok(response.is_success().then_some(response.body))
        };
        let missing =
            |name: &str| CoreError::NotFound { message: format!("{repo}'s latest release has no {name}") };
        let manifest = release("manifest.json")?.ok_or_else(|| missing("manifest.json"))?;
        let main = release("main.js")?.ok_or_else(|| missing("main.js"))?;
        let styles = release("styles.css")?;
        let id = self.lock()?.install_obsidian_plugin(&manifest, &main, styles.as_deref())?.id;
        self.obsidian_plugins()?.into_iter().find(|p| p.id == id).ok_or_else(|| CoreError::NotFound {
            message: format!("The plugin {id} is missing after installing it."),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use paperloom_http::{BytesResponse, NetworkError, Response};

    use super::*;

    /// A request as the site saw it: method, address, headers, body.
    type Asked = (String, String, Vec<(String, String)>, Option<Vec<u8>>);

    /// Answers every request and keeps what was asked.
    #[derive(Default)]
    struct Site {
        asked: Mutex<Vec<Asked>>,
    }

    impl Http for Site {
        fn get(&self, _url: &str, _accept: &str) -> Result<Response, NetworkError> {
            Err(NetworkError("not used".into()))
        }

        fn post_form(&self, _url: &str, _form: &[(&str, &str)]) -> Result<Response, NetworkError> {
            Err(NetworkError("not used".into()))
        }

        fn send(&self, request: Request) -> Result<BytesResponse, NetworkError> {
            self.asked.lock().unwrap().push((
                request.method.into(),
                request.url,
                request.headers,
                request.body,
            ));
            Ok(BytesResponse { status: 201, body: "créé".as_bytes().to_vec() })
        }
    }

    /// A web where each address has one answer.
    struct Web(Vec<(&'static str, u16, &'static str)>);

    impl Http for Web {
        fn get(&self, url: &str, _accept: &str) -> Result<Response, NetworkError> {
            Ok(self.0.iter().find(|(address, _, _)| *address == url).map_or_else(
                || Response::new(404, "Not Found"),
                |(_, status, body)| Response::new(*status, *body),
            ))
        }

        fn post_form(&self, _url: &str, _form: &[(&str, &str)]) -> Result<Response, NetworkError> {
            Err(NetworkError("not used".into()))
        }
    }

    #[test]
    fn installs_community_plugins_from_their_releases() {
        let web = Web(vec![
            (
                CATALOG,
                200,
                r#"[{"id": "dataview", "name": "Dataview", "author": "Michael", "description": "Queries.",
                    "repo": "blacksmithgu/obsidian-dataview"}]"#,
            ),
            (
                "https://github.com/blacksmithgu/obsidian-dataview/releases/latest/download/manifest.json",
                200,
                r#"{"id": "dataview", "name": "Dataview", "version": "0.5.68"}"#,
            ),
            (
                "https://github.com/blacksmithgu/obsidian-dataview/releases/latest/download/main.js",
                200,
                "class X {}",
            ),
        ]);
        let listed = catalog_with(&web).unwrap();
        assert_eq!(listed[0].repo, "blacksmithgu/obsidian-dataview");
        let dir = tempfile::tempdir().unwrap();
        let vault = PaperVault::open(dir.path().join("Vault").display().to_string()).unwrap();
        let installed = vault.install_with(&web, "blacksmithgu/obsidian-dataview").unwrap();
        assert_eq!(
            (installed.id.as_str(), installed.version.as_str(), installed.enabled),
            ("dataview", "0.5.68", false)
        );
        assert!(!installed.has_styles, "the release has no styles.css");
        assert_eq!(vault.obsidian_plugin_code("dataview".into()).unwrap().main, "class X {}");
        assert!(matches!(vault.install_with(&web, "nobody/nothing"), Err(CoreError::NotFound { .. })));
        assert!(vault.install_with(&web, "../../etc").is_err());
        assert!(vault.install_with(&web, "a/b/c").is_err());
    }

    #[test]
    fn fetches_web_addresses_only() {
        let site = Site::default();
        let header = PluginHeader { name: "Accept".into(), value: "application/json".into() };
        let answer =
            fetch_with(&site, "https://api.example.org/x".into(), "post", vec![header], Some("{}".into()))
                .unwrap();
        assert_eq!(answer, PluginResponse { status: 201, text: "créé".into() });
        let asked = site.asked.lock().unwrap();
        assert_eq!(asked[0].0, "POST");
        assert_eq!(asked[0].2, [("Accept".to_string(), "application/json".to_string())]);
        assert_eq!(asked[0].3.as_deref(), Some(b"{}".as_slice()));
        drop(asked);
        assert!(fetch_with(&site, "file:///etc/passwd".into(), "GET", vec![], None).is_err());
        assert!(fetch_with(&site, "https://x.org".into(), "CONNECT", vec![], None).is_err());
    }

    #[test]
    fn plugins_go_through_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Vault");
        let folder = root.join(".obsidian/plugins/word-count");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("manifest.json"),
            r#"{"id": "word-count", "name": "Word Count", "version": "1.0"}"#,
        )
        .unwrap();
        std::fs::write(folder.join("main.js"), "module.exports = {};").unwrap();
        let vault = PaperVault::open(root.display().to_string()).unwrap();
        let plugins = vault.obsidian_plugins().unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!((plugins[0].id.as_str(), plugins[0].enabled), ("word-count", false));
        vault.set_obsidian_plugin_enabled("word-count".into(), true).unwrap();
        assert!(vault.obsidian_plugins().unwrap()[0].enabled);
        vault.save_obsidian_plugin_data("word-count".into(), "{}".into()).unwrap();
        assert_eq!(vault.obsidian_plugin_code("word-count".into()).unwrap().data.as_deref(), Some("{}"));
    }
}
