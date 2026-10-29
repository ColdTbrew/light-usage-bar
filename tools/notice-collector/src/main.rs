use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
    process::ExitCode,
};

const MAX_METADATA: u64 = 64 * 1024 * 1024;
const MAX_FILE: u64 = 8 * 1024 * 1024;
const MAX_TOTAL: u64 = 512 * 1024 * 1024;
type Result<T> = std::result::Result<T, String>;

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string {key}"))
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing array {key}"))
}
fn relative(path: &str) -> Result<PathBuf> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("license path must contain only normal relative components".into());
    }
    Ok(path.to_owned())
}
fn notice_name(path: &Path) -> bool {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_uppercase();
    ["LICENSE", "COPYING", "NOTICE"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}
fn discover(
    root: &Path,
    directory: &Path,
    depth: usize,
    count: &mut usize,
    files: &mut BTreeSet<PathBuf>,
    issues: &mut BTreeSet<String>,
) -> Result<()> {
    if depth > 16 {
        return Err("source directory depth exceeds 16".into());
    }
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        *count += 1;
        if *count > 100_000 {
            return Err("source entry count exceeds 100000".into());
        }
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            issues.insert(format!(
                "symlink skipped: {}",
                path.strip_prefix(root).unwrap().display()
            ));
        } else if kind.is_dir() {
            discover(root, &path, depth + 1, count, files, issues)?;
        } else if kind.is_file()
            && path
                .strip_prefix(root)
                .unwrap()
                .components()
                .any(|component| notice_name(Path::new(component.as_os_str())))
        {
            files.insert(path.strip_prefix(root).unwrap().to_owned());
        }
    }
    Ok(())
}
fn regular_file(root: &Path, relative: &Path) -> Result<PathBuf> {
    let mut path = root.to_owned();
    for component in relative.components() {
        path.push(component);
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("symlink in license path".into());
        }
    }
    if !fs::metadata(&path).map_err(|e| e.to_string())?.is_file() {
        return Err("license is not a regular file".into());
    }
    Ok(path)
}
fn collect(metadata: &Value, output: &Path, supplemental: Option<&Path>) -> Result<bool> {
    let supplements = if let Some(root) = supplemental {
        let mut bytes = Vec::new();
        fs::File::open(root.join("manifest.json"))
            .map_err(|e| e.to_string())?
            .take(MAX_METADATA + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_METADATA {
            return Err("supplemental manifest too large".into());
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        value
            .as_array()
            .ok_or("supplemental manifest must be an array")?
            .clone()
    } else {
        Vec::new()
    };
    let packages: BTreeMap<_, _> = array(metadata, "packages")?
        .iter()
        .map(|p| Ok((string(p, "id")?.to_owned(), p)))
        .collect::<Result<_>>()?;
    if packages.len() > 10_000 {
        return Err("package count exceeds 10000".into());
    }
    let workspace: BTreeSet<_> = array(metadata, "workspace_members")?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or("invalid workspace member".into())
        })
        .collect::<Result<_>>()?;
    let resolve = metadata.get("resolve").ok_or("missing resolve graph")?;
    let nodes: BTreeMap<_, _> = array(resolve, "nodes")?
        .iter()
        .map(|n| Ok((string(n, "id")?.to_owned(), n)))
        .collect::<Result<_>>()?;
    let mut pending = Vec::new();
    for name in ["usage-app", "usage-panel"] {
        let roots: Vec<_> = packages
            .iter()
            .filter(|(id, p)| {
                workspace.contains(*id) && p.get("name").and_then(Value::as_str) == Some(name)
            })
            .map(|(id, _)| id.clone())
            .collect();
        if roots.len() != 1 {
            return Err(format!("expected one workspace root {name}"));
        }
        pending.extend(roots);
    }
    let mut reached = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !reached.insert(id.clone()) {
            continue;
        }
        let node = nodes
            .get(&id)
            .ok_or_else(|| format!("missing resolve node {id}"))?;
        for dependency in array(node, "dependencies")? {
            pending.push(
                dependency
                    .as_str()
                    .ok_or("invalid dependency id")?
                    .to_owned(),
            );
        }
    }
    if output.exists() {
        return Err("output directory must not already exist".into());
    }
    fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    let mut total = 0;
    let mut unresolved = false;
    for id in reached.difference(&workspace) {
        let package = packages
            .get(id)
            .ok_or_else(|| format!("missing package {id}"))?;
        let name = string(package, "name")?;
        let version = string(package, "version")?;
        let source = package.get("source").and_then(Value::as_str);
        let mut issues = BTreeSet::new();
        let mut files = BTreeSet::new();
        let folder = format!("packages/{:04}", entries.len());
        let mut copied = Vec::new();
        let mut supplemental_sources = Vec::new();
        if source.is_some_and(|s| s.starts_with("registry+")) {
            let manifest = Path::new(string(package, "manifest_path")?);
            let root = manifest.parent().ok_or("manifest has no parent")?;
            if fs::symlink_metadata(root)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
            {
                return Err("crate source root is a symlink".into());
            }
            discover(root, root, 0, &mut 0, &mut files, &mut issues)?;
            if let Some(explicit) = package.get("license_file").and_then(Value::as_str) {
                match relative(explicit).and_then(|p| regular_file(root, &p).map(|_| p)) {
                    Ok(path) => {
                        files.insert(path);
                    }
                    Err(error) => {
                        issues.insert(format!("invalid license_file: {error}"));
                    }
                }
            }
            for file in files {
                let source_path = regular_file(root, &file)?;
                let bytes = fs::metadata(&source_path).map_err(|e| e.to_string())?.len();
                if bytes > MAX_FILE || total + bytes > MAX_TOTAL {
                    return Err("license byte limit exceeded".into());
                }
                total += bytes;
                let destination = Path::new(&folder).join(&file);
                let target = output.join(&destination);
                fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
                let mut data = Vec::new();
                fs::File::open(&source_path)
                    .map_err(|e| e.to_string())?
                    .take(MAX_FILE + 1)
                    .read_to_end(&mut data)
                    .map_err(|e| e.to_string())?;
                if data.len() as u64 != bytes {
                    return Err("license file changed during collection".into());
                }
                fs::write(target, data).map_err(|e| e.to_string())?;
                copied.push(
                    destination
                        .to_str()
                        .ok_or("non-UTF8 license path")?
                        .to_owned(),
                );
            }
            if copied.is_empty()
                && let Some(root) = supplemental
            {
                let matches: Vec<_> = supplements
                    .iter()
                    .filter(|p| p["name"] == name && p["version"] == version)
                    .collect();
                if matches.len() > 1 {
                    return Err("duplicate supplemental package".into());
                }
                if let Some(entry) = matches.first() {
                    for file in array(entry, "files")? {
                        let path = relative(string(file, "path")?)?;
                        let source_path = regular_file(root, &path)?;
                        let source_url = string(file, "source_url")?;
                        if !source_url.starts_with("https://") {
                            return Err("supplemental source must use HTTPS".into());
                        }
                        let mut bytes = Vec::new();
                        fs::File::open(source_path)
                            .map_err(|e| e.to_string())?
                            .take(MAX_FILE + 1)
                            .read_to_end(&mut bytes)
                            .map_err(|e| e.to_string())?;
                        if bytes.len() as u64 > MAX_FILE || total + bytes.len() as u64 > MAX_TOTAL {
                            return Err("license byte limit exceeded".into());
                        }
                        let digest = format!("{:x}", Sha256::digest(&bytes));
                        if digest != string(file, "sha256")? {
                            return Err(format!("supplemental checksum mismatch for {name}"));
                        }
                        total += bytes.len() as u64;
                        let destination = Path::new(&folder).join("supplemental").join(&path);
                        let target = output.join(&destination);
                        fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
                        fs::write(target, bytes).map_err(|e| e.to_string())?;
                        copied.push(
                            destination
                                .to_str()
                                .ok_or("non-UTF8 license path")?
                                .to_owned(),
                        );
                        supplemental_sources.push(
                            json!({"url":source_url,"sha256":digest,"reason":entry.get("reason")}),
                        );
                    }
                }
            }
            if copied.is_empty() {
                issues.insert("no license or notice files found".into());
            }
        } else {
            issues
                .insert("non-registry dependency requires manual source and notice review".into());
        }
        if package.get("license").and_then(Value::as_str).is_none() {
            issues.insert("missing license expression".into());
        }
        unresolved |= !issues.is_empty();
        let crates_io = source == Some("registry+https://github.com/rust-lang/crates.io-index")
            || source == Some("registry+sparse+https://index.crates.io/");
        entries.push(json!({"id":id,"name":name,"version":version,"license":package.get("license"),"repository":package.get("repository"),"source":source,"source_archive":crates_io.then(||format!("https://crates.io/api/v1/crates/{name}/{version}/download")),"source_page":crates_io.then(||format!("https://crates.io/crates/{name}/{version}")),"files":copied,"supplemental_sources":supplemental_sources,"unresolved":issues.into_iter().collect::<Vec<_>>()}));
    }
    let index = json!({"format_version":1,"roots":["usage-app","usage-panel"],"scope":"All dependencies reachable in the supplied target-filtered Cargo metadata graph, including build and development dependencies. Workspace code is excluded. Collection does not establish license compliance or satisfy source-distribution obligations.","unresolved":unresolved,"packages":entries});
    let mut bytes = serde_json::to_vec_pretty(&index).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    fs::write(output.join("index.json"), bytes).map_err(|e| e.to_string())?;
    Ok(unresolved)
}
fn run() -> Result<bool> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if !(2..=3).contains(&args.len()) {
        return Err(
            "usage: notice-collector METADATA.json OUTPUT_DIR [SUPPLEMENTAL_DIR] (metadata may be - for stdin)".into(),
        );
    }
    let reader: Box<dyn Read> = if args[0] == "-" {
        Box::new(io::stdin())
    } else {
        Box::new(fs::File::open(&args[0]).map_err(|e| e.to_string())?)
    };
    let mut bytes = Vec::new();
    reader
        .take(MAX_METADATA + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_METADATA {
        return Err("metadata exceeds 64 MiB".into());
    }
    let metadata: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    collect(&metadata, Path::new(&args[1]), args.get(2).map(Path::new))
}
fn main() -> ExitCode {
    match run() {
        Ok(false) => ExitCode::SUCCESS,
        Ok(true) => {
            eprintln!("notice collection has unresolved entries; inspect index.json");
            ExitCode::from(2)
        }
        Err(error) => {
            eprintln!("notice collection failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = env::temp_dir().join(format!(
                "notice-collector-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            fs::create_dir(root.join("crate")).unwrap();
            Self(root)
        }
        fn metadata(&self) -> Value {
            let mut packages = Vec::new();
            let mut nodes = Vec::new();
            for name in ["usage-app", "usage-panel", "dep"] {
                packages.push(json!({"id":name,"name":name,"version":"1.2.3","license":"MIT","repository":"https://example.org/source","source":if name == "dep" { Some("registry+https://github.com/rust-lang/crates.io-index") } else { None },"manifest_path":self.0.join("crate/Cargo.toml").to_str().unwrap(),"license_file":null}));
                nodes.push(json!({"id":name,"dependencies":if name == "dep" { vec![] } else { vec!["dep"] }}));
            }
            json!({"packages":packages,"workspace_members":["usage-app","usage-panel"],"resolve":{"nodes":nodes}})
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn deterministic_output_contains_nested_notices_and_source_metadata() {
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join("crate/licenses")).unwrap();
        fs::write(
            fixture.0.join("crate/licenses/LICENSE-MIT"),
            "license text\n",
        )
        .unwrap();
        fs::write(fixture.0.join("crate/NOTICE"), "notice\n").unwrap();
        fs::write(
            fixture.0.join("crate/licenses/Apache-2.0.txt"),
            "other text\n",
        )
        .unwrap();
        let first = fixture.0.join("one");
        let second = fixture.0.join("two");
        assert!(!collect(&fixture.metadata(), &first, None).unwrap());
        assert!(!collect(&fixture.metadata(), &second, None).unwrap());
        assert_eq!(
            fs::read(first.join("index.json")).unwrap(),
            fs::read(second.join("index.json")).unwrap()
        );
        assert_eq!(
            fs::read(first.join("packages/0000/licenses/LICENSE-MIT")).unwrap(),
            b"license text\n"
        );
        let index: Value =
            serde_json::from_slice(&fs::read(first.join("index.json")).unwrap()).unwrap();
        assert_eq!(index["packages"].as_array().unwrap().len(), 1);
        assert_eq!(index["packages"][0]["files"].as_array().unwrap().len(), 3);
        assert_eq!(
            index["packages"][0]["source_archive"],
            "https://crates.io/api/v1/crates/dep/1.2.3/download"
        );
        assert!(
            collect(&fixture.metadata(), &first, None)
                .unwrap_err()
                .contains("already exist")
        );
    }
    #[test]
    fn supplemental_notices_require_matching_versions_and_checksums() {
        let fixture = Fixture::new();
        let supplemental = fixture.0.join("supplemental");
        fs::create_dir(&supplemental).unwrap();
        fs::write(supplemental.join("LICENSE"), "upstream notice").unwrap();
        let mut manifest = json!([{"name":"dep","version":"1.2.3","reason":"published source omitted notice", "files":[{"path":"LICENSE","source_url":"https://example.org/immutable/LICENSE","sha256":format!("{:x}",Sha256::digest(b"upstream notice"))}]}]);
        fs::write(supplemental.join("manifest.json"), manifest.to_string()).unwrap();
        assert!(
            !collect(
                &fixture.metadata(),
                &fixture.0.join("good"),
                Some(&supplemental)
            )
            .unwrap()
        );
        manifest[0]["files"][0]["sha256"] = json!("invalid");
        fs::write(supplemental.join("manifest.json"), manifest.to_string()).unwrap();
        assert!(
            collect(
                &fixture.metadata(),
                &fixture.0.join("bad"),
                Some(&supplemental)
            )
            .unwrap_err()
            .contains("checksum mismatch")
        );
        manifest[0]["version"] = json!("9.9.9");
        fs::write(supplemental.join("manifest.json"), manifest.to_string()).unwrap();
        assert!(
            collect(
                &fixture.metadata(),
                &fixture.0.join("wrong-version"),
                Some(&supplemental)
            )
            .unwrap()
        );
    }

    #[test]
    fn missing_license_and_traversal_are_unresolved() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("SECRET"), "outside source").unwrap();
        let mut metadata = fixture.metadata();
        metadata["packages"][2]["license_file"] = json!("../SECRET");
        let output = fixture.0.join("out");
        assert!(collect(&metadata, &output, None).unwrap());
        let index: Value =
            serde_json::from_slice(&fs::read(output.join("index.json")).unwrap()).unwrap();
        assert!(index["packages"][0]["files"].as_array().unwrap().is_empty());
        assert_eq!(
            index["packages"][0]["unresolved"].as_array().unwrap().len(),
            2
        );
        assert!(!output.join("SECRET").exists());
    }
    #[cfg(unix)]
    #[test]
    fn symlinks_never_copy_external_files_or_directories() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        fs::write(fixture.0.join("SECRET"), "outside source").unwrap();
        symlink(fixture.0.join("SECRET"), fixture.0.join("crate/LICENSE")).unwrap();
        symlink(&fixture.0, fixture.0.join("crate/licenses")).unwrap();
        let mut metadata = fixture.metadata();
        metadata["packages"][2]["license_file"] = json!("licenses/SECRET");
        let output = fixture.0.join("out");
        assert!(collect(&metadata, &output, None).unwrap());
        let index: Value =
            serde_json::from_slice(&fs::read(output.join("index.json")).unwrap()).unwrap();
        assert!(index["packages"][0]["files"].as_array().unwrap().is_empty());
        assert_eq!(
            index["packages"][0]["unresolved"].as_array().unwrap().len(),
            4
        );
    }
}
