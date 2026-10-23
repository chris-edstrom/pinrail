//! The official plugins the app carries, offered to the person to install.
//! Nothing in the catalog is installed until the person chooses it.
//!
//! The catalog is an index in the registry's shape: each plugin is known by
//! its id, `forgeplane/<name>`, and its version, and an install from it
//! records the same as an install from the registry's index will. The
//! registry later adds a second catalog, fetched rather than carried, and
//! the version a plugin can be updated to is the highest any of them offers.

use include_dir::{Dir, include_dir};
use pinrail_format::bundle::{Listing, Taken};
use serde_json::{Map, Value, json};

use super::bundles::Files;

/// The plugins the app carries. They live in `plugins/` with the test
/// fixtures; build.rs copies the catalog's bundles here for the binary.
static CATALOG: Dir = include_dir!("$OUT_DIR/catalog");

/// The publisher of the plugins the app carries.
pub const PUBLISHER: &str = "forgeplane";

/// The repository the official plugins come from.
const REPOSITORY: &str = "https://github.com/forgeplane/pinrail";

/// The plugins the setup wizard selects.
const RECOMMENDED: &[&str] = &["list", "feedback"];

/// One plugin a catalog offers, with the files an install stores.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub version: String,
    /// The plugin's files, paths relative to the plugin, as a bundle holds
    /// them.
    pub files: Files,
    /// The hash of those files as a bundle: what an install of the entry
    /// records as its bundle.
    pub hash: String,
    manifest: Map<String, Value>,
    icon: Option<String>,
}

impl Entry {
    fn of(files: Files) -> Result<Entry, String> {
        let manifest = files
            .iter()
            .find(|(path, _)| path == "manifest.json")
            .and_then(|(_, bytes)| serde_json::from_slice::<Value>(bytes).ok())
            .and_then(|value| value.as_object().cloned())
            .ok_or("no manifest.json")?;
        let name = manifest
            .get("name")
            .and_then(Value::as_str)
            .ok_or("no name in manifest.json")?
            .to_string();
        let version = manifest
            .get("version")
            .and_then(super::version_of)
            .map(|(version, _)| version)
            .ok_or("no version in manifest.json")?;
        let hash = Listing::from_files(
            files.iter().map(|(p, b)| (p.as_str(), b.as_slice())),
            Taken::AsBundle,
        )?
        .hash();
        let icon = files
            .iter()
            .find(|(path, _)| path == pinrail_format::manifest::ICON)
            .and_then(|(path, bytes)| {
                let text = String::from_utf8_lossy(bytes);
                pinrail_format::manifest::svg_markup(path, &text).ok()
            });
        Ok(Entry {
            name,
            version,
            files,
            hash,
            manifest,
            icon,
        })
    }

    /// The Pinrail version the plugin needs, when this one is older.
    pub fn needs(&self) -> Option<String> {
        let needed = self.manifest.get("pinrail")?.as_str()?;
        let needed = needed.trim_start_matches(">=").trim();
        let this = env!("CARGO_PKG_VERSION");
        (pinrail_format::semver(this) < pinrail_format::semver(needed)).then(|| needed.to_string())
    }

    /// The registry's id: `<publisher>/<name>`.
    pub fn id(&self) -> String {
        format!("{PUBLISHER}/{}", self.name)
    }

    /// The entry as the registry's compiled index lists a plugin. The
    /// catalog's zips are its files, so `url` is null and `sha256` is the
    /// bundle's hash.
    pub fn to_json(&self) -> Value {
        let field = |key: &str| self.manifest.get(key).cloned().unwrap_or(Value::Null);
        json!({
            "id": self.id(),
            "publisher": PUBLISHER,
            "name": self.name,
            "official": true,
            "recommended": RECOMMENDED.contains(&self.name.as_str()),
            "repository": REPOSITORY,
            "version": self.version,
            "url": null,
            "sha256": self.hash,
            "size": self.files.iter().map(|(_, b)| b.len()).sum::<usize>(),
            "pinrail": field("pinrail"),
            "title": self.manifest.get("title").cloned().unwrap_or_else(|| self.name.clone().into()),
            "description": field("description"),
            "use_when": field("use_when"),
            "icon": self.icon,
            "attachments": field("attachments"),
        })
    }
}

/// The plugins one source offers, by name.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    entries: Vec<Entry>,
}

impl Catalog {
    /// The plugins this app carries.
    pub fn builtin() -> Catalog {
        Catalog::of(carried())
    }

    /// A catalog of the plugin folders in `dir`, each as a bundle holds it.
    /// A folder that is not a plugin is left out.
    pub fn of_dir(dir: &std::path::Path) -> std::io::Result<Catalog> {
        let mut plugins = Vec::new();
        for entry in std::fs::read_dir(dir)?.flatten() {
            let folder = entry.path();
            if !folder.join("manifest.json").is_file() {
                continue;
            }
            let Ok(listing) = Listing::of_folder(&folder, Taken::FromSource) else {
                continue;
            };
            let files = listing
                .files
                .iter()
                .map(|f| Ok((f.path.clone(), std::fs::read(folder.join(&f.path))?)))
                .collect::<std::io::Result<Files>>()?;
            plugins.push((entry.file_name().to_string_lossy().into_owned(), files));
        }
        Ok(Catalog::of(plugins))
    }

    /// A catalog of these plugins' files, each by its folder. A plugin
    /// whose files are not a bundle is left out.
    pub fn of(plugins: Vec<(String, Files)>) -> Catalog {
        let mut entries: Vec<Entry> = plugins
            .into_iter()
            .filter_map(|(_, files)| Entry::of(files).ok())
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Catalog { entries }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
}

/// The entry for `id`, `forgeplane/<name>` or the bare name, at the highest
/// version any of the catalogs offers.
pub fn best<'a>(catalogs: &'a [Catalog], id: &str) -> Option<&'a Entry> {
    let name = match id.split_once('/') {
        Some((PUBLISHER, name)) => name,
        Some(_) => return None,
        None => id,
    };
    catalogs
        .iter()
        .flat_map(|c| c.entries.iter())
        .filter(|e| e.name == name)
        .max_by(|a, b| pinrail_format::semver(&a.version).cmp(&pinrail_format::semver(&b.version)))
}

/// The entry for `id` at exactly `version`, from whichever catalog offers
/// it.
pub fn at<'a>(catalogs: &'a [Catalog], id: &str, version: &str) -> Option<&'a Entry> {
    let name = match id.split_once('/') {
        Some((PUBLISHER, name)) => name,
        Some(_) => return None,
        None => id,
    };
    catalogs
        .iter()
        .flat_map(|c| c.entries.iter())
        .find(|e| e.name == name && e.version == version)
}

/// Every plugin the catalogs offer, each at its highest version, by name.
pub fn listing(catalogs: &[Catalog]) -> Vec<&Entry> {
    let mut names: Vec<&str> = catalogs
        .iter()
        .flat_map(|c| c.entries.iter().map(|e| e.name.as_str()))
        .collect();
    names.sort_unstable();
    names.dedup();
    names
        .into_iter()
        .filter_map(|name| best(catalogs, name))
        .collect()
}

/// The files of each plugin the app carries, by its folder: paths relative
/// to the plugin, and their bytes.
pub(crate) fn carried() -> Vec<(String, Files)> {
    fn files(dir: &Dir, root: &std::path::Path, out: &mut Files) {
        for file in dir.files() {
            let path = file.path().strip_prefix(root).unwrap_or(file.path());
            out.push((
                path.to_string_lossy().replace('\\', "/"),
                file.contents().to_vec(),
            ));
        }
        for child in dir.dirs() {
            files(child, root, out);
        }
    }
    CATALOG
        .dirs()
        .map(|plugin| {
            let mut out = Vec::new();
            files(plugin, plugin.path(), &mut out);
            (plugin.path().to_string_lossy().into_owned(), out)
        })
        .collect()
}

/// The version of a plugin the app carries, by its folder. Tests offer
/// releases relative to it, with [`later`] and [`next_major`], so that a
/// plugin's release does not change what they check.
#[cfg(test)]
pub(crate) fn carried_version(folder: &str) -> String {
    let (_, files) = carried().into_iter().find(|(f, _)| f == folder).unwrap();
    let (_, manifest) = files
        .into_iter()
        .find(|(path, _)| path == "manifest.json")
        .unwrap();
    serde_json::from_slice::<serde_json::Value>(&manifest).unwrap()["version"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// The version `minors` minor releases after `version`.
#[cfg(test)]
pub(crate) fn later(version: &str, minors: u64) -> String {
    let (major, minor, _) = pinrail_format::semver(version);
    format!("{major}.{}.0", minor + minors)
}

/// The first version of the major release after `version`'s.
#[cfg(test)]
pub(crate) fn next_major(version: &str) -> String {
    format!("{}.0.0", pinrail_format::semver(version).0 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `list` from the app's catalog at another version.
    fn list_at(version: &str) -> Catalog {
        let files = carried()
            .into_iter()
            .find(|(folder, _)| folder == "list")
            .unwrap()
            .1
            .into_iter()
            .map(|(path, bytes)| match path.as_str() {
                "manifest.json" => {
                    let mut manifest: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    manifest["version"] = version.into();
                    (path, serde_json::to_vec(&manifest).unwrap())
                }
                _ => (path, bytes),
            })
            .collect();
        Catalog::of(vec![("list".into(), files)])
    }

    #[test]
    fn the_app_carries_its_official_plugins_and_recommends_list_and_feedback() {
        let catalog = Catalog::builtin();
        let rows: Vec<Value> = catalog.entries().iter().map(Entry::to_json).collect();
        // every plugin folder the app carries, by name
        let names: Vec<&str> = rows.iter().map(|r| r["name"].as_str().unwrap()).collect();
        let mut carried: Vec<String> = carried().into_iter().map(|(folder, _)| folder).collect();
        carried.sort();
        assert_eq!(names, carried);
        for row in &rows {
            assert_eq!(
                row["id"],
                format!("forgeplane/{}", row["name"].as_str().unwrap())
            );
        }
        for row in &rows {
            assert_eq!(row["official"], true);
            assert_eq!(
                row["recommended"],
                row["name"] == "list" || row["name"] == "feedback"
            );
            assert!(row["icon"].as_str().unwrap().starts_with("<svg"));
            assert!(row["use_when"].is_string());
            assert_eq!(row["sha256"].as_str().unwrap().len(), 64);
        }
    }

    /// Every plugin the app carries loads with all it declares: a setting,
    /// a template or a skill the loader dropped would be lost on everyone's
    /// machine.
    #[test]
    fn every_carried_plugin_loads_without_dropping_a_feature() {
        for (folder, files) in carried() {
            let dir = tempfile::tempdir().unwrap();
            for (path, bytes) in &files {
                let to = dir.path().join(path);
                std::fs::create_dir_all(to.parent().unwrap()).unwrap();
                std::fs::write(to, bytes).unwrap();
            }
            let plugin = crate::plugins::Plugin::load(dir.path());
            assert!(plugin.usable(), "{folder}: {:?}", plugin.error);
            assert_eq!(
                plugin.verdict()["warnings"],
                serde_json::json!([]),
                "{folder}"
            );
        }
    }

    /// The change map carries a skill for agents, which loads whole.
    #[test]
    fn the_change_map_carries_its_skill() {
        let (_, files) = carried()
            .into_iter()
            .find(|(f, _)| f == "change-map")
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        for (path, bytes) in &files {
            let to = dir.path().join(path);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::write(to, bytes).unwrap();
        }
        let plugin = crate::plugins::Plugin::load(dir.path());
        assert!(plugin.usable(), "{:?}", plugin.error);
        assert_eq!(plugin.skill_error, None);
        let skill = plugin.skill.unwrap();
        assert!(
            skill.description.starts_with("Build a change map"),
            "{}",
            skill.description
        );
        assert_eq!(
            skill.files,
            [
                "SKILL.md",
                "references/diagrams.md",
                "references/screenshots.md",
                "scripts/check.mjs",
                "scripts/files.mjs"
            ]
        );
    }

    /// What the app carries is what a plugin's folder holds as a bundle.
    #[test]
    fn a_carried_plugin_ships_its_bundle_and_nothing_else() {
        for (folder, files) in carried() {
            let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../plugins")
                .join(&folder);
            let bundle = Listing::of_folder(&source, Taken::FromSource).unwrap();
            let shipped = Listing::from_files(
                files.iter().map(|(p, b)| (p.as_str(), b.as_slice())),
                Taken::AsBundle,
            );
            assert_eq!(shipped, Ok(bundle), "{folder}");
        }
    }

    #[test]
    fn the_highest_version_any_catalog_offers_wins() {
        let newer = later(&carried_version("list"), 1);
        let older = "0.0.1";
        let catalogs = [Catalog::builtin(), list_at(&newer), list_at(older)];
        assert_eq!(best(&catalogs, "list").unwrap().version, newer);
        assert_eq!(best(&catalogs, "forgeplane/list").unwrap().version, newer);
        assert_eq!(
            best(&catalogs, "feedback").unwrap().version,
            carried_version("feedback")
        );
        assert!(best(&catalogs, "acme/list").is_none());
        assert!(best(&catalogs, "nothing").is_none());
        // each plugin once, at its highest version, by name
        let listed = listing(&catalogs);
        let names: Vec<&str> = listed.iter().map(|e| e.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names, sorted);
        let list = listed.iter().find(|e| e.name == "list").unwrap();
        assert_eq!(list.version, newer);
        assert_eq!(at(&catalogs, "list", older).unwrap().version, older);
        assert!(at(&catalogs, "list", &later(&newer, 1)).is_none());
    }

    /// A folder of plugin folders, as tests give one, offers each in the
    /// version its manifest says, and skips what is not a plugin.
    #[test]
    fn a_folder_of_plugins_is_a_catalog() {
        let dir = tempfile::tempdir().unwrap();
        for (folder, files) in carried() {
            for (path, bytes) in files {
                let target = dir.path().join(&folder).join(path);
                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                std::fs::write(target, bytes).unwrap();
            }
        }
        std::fs::create_dir_all(dir.path().join("not-a-plugin")).unwrap();
        let catalog = Catalog::of_dir(dir.path()).unwrap();
        let builtin = Catalog::builtin();
        let names = |c: &Catalog| {
            c.entries()
                .iter()
                .map(|e| e.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&catalog), names(&builtin));
        let hash = |c: &Catalog| {
            c.entries()
                .iter()
                .find(|e| e.name == "list")
                .unwrap()
                .hash
                .clone()
        };
        assert_eq!(hash(&catalog), hash(&builtin));
    }
}
