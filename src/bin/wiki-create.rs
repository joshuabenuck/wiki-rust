use clap::{App, AppSettings, Arg};
use failure::{err_msg, Error};
use glob::glob;
use log::debug;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_yaml;
use std::fs::{create_dir_all, remove_dir_all, write, File};
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{exit, Command};
use tar::Archive;
use url::Url;
use xz2::read::XzDecoder;

#[derive(PartialEq)]
struct Branch {
    path_spec: String,
}

impl Branch {
    fn user_repo_branch(&self) -> (&str, &str, &str) {
        let path_spec = &self.path_spec;
        let mut parts = path_spec.split(":");
        let user_repo = parts.next().unwrap();
        let mut user_repo_parts = user_repo.split("/");
        let user = user_repo_parts
            .next()
            .expect(format!("Unable to find user in {}", path_spec).as_str());
        let repo = user_repo_parts
            .next()
            .expect(format!("Unable to find repo in {}", path_spec).as_str());
        let branch = parts.next().unwrap_or("master");
        (user, repo, branch)
    }

    fn url(&self) -> Url {
        let (user, repo, branch) = self.user_repo_branch();
        Url::parse(
            format!(
                "https://github.com/{}/{}/archive/{}.zip",
                user, repo, branch
            )
            .as_str(),
        )
        .expect("Unable to parse wiki url")
    }

    fn dir(&self) -> PathBuf {
        let (_user, repo, branch) = self.user_repo_branch();
        PathBuf::from(format!("{}-{}", repo, branch))
    }

    fn zip(&self) -> PathBuf {
        let (_user, repo, branch) = self.user_repo_branch();
        PathBuf::from(format!("{}-{}.zip", repo, branch))
    }
}

impl Serialize for Branch {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.path_spec.as_str())
    }
}

impl<'de> Deserialize<'de> for Branch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Branch {
            path_spec: Deserialize::deserialize(deserializer)?,
        })
    }
}

impl From<&str> for Branch {
    fn from(path_spec: &str) -> Self {
        Branch {
            path_spec: path_spec.to_owned(),
        }
    }
}

impl Default for Branch {
    fn default() -> Self {
        Branch {
            path_spec: "".to_string(),
        }
    }
}

#[derive(Deserialize, Serialize, PartialEq)]
struct NodeConfig {
    url: Option<String>,
    path: Option<PathBuf>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        NodeConfig {
            url: None,
            path: None,
        }
    }
}

#[derive(Deserialize, Serialize)]
struct WikiConfig {
    #[serde(default, skip_serializing_if = "is_default")]
    dir: PathBuf,
    #[serde(default, skip_serializing_if = "is_default")]
    wiki: Branch,
    #[serde(default, skip_serializing_if = "is_default")]
    server: Branch,
    #[serde(default, skip_serializing_if = "is_default")]
    client: Branch,
    #[serde(default, skip_serializing_if = "is_default")]
    plugins: Vec<Branch>,
    #[serde(default, skip_serializing_if = "is_default")]
    node: NodeConfig,
}

fn is_default<T: Default + PartialEq>(t: &T) -> bool {
    t == &T::default()
}

impl Default for WikiConfig {
    fn default() -> Self {
        WikiConfig {
            dir: dirs::home_dir()
                .expect("Unable to find home dir.")
                .join("wiki"),
            wiki: "fedwiki/wiki".into(),
            server: "fedwiki/wiki-server".into(),
            client: "fedwiki/wiki-client".into(),
            plugins: Vec::new(),
            node: NodeConfig {
                url: None,
                path: None,
            },
        }
    }
}

impl WikiConfig {
    fn from_config(config: &str) -> Result<WikiConfig, Error> {
        let config = config.replace("~", dirs::home_dir().unwrap().to_str().unwrap());
        let reader = File::open(config)?;
        let config = serde_yaml::from_reader(reader)?;
        Ok(config)
    }

    fn new(dir: &str) -> WikiConfig {
        WikiConfig {
            dir: dir.into(),
            ..WikiConfig::default()
        }
    }

    fn canonical_dir(&self) -> PathBuf {
        self.dir
            .to_str()
            .unwrap()
            .replace("~", dirs::home_dir().unwrap().to_str().unwrap())
            .into()
    }

    fn exists(&self) -> bool {
        self.canonical_dir().join("config.yaml").exists()
    }

    fn create_folder(&self) -> Result<(), io::Error> {
        create_dir_all(self.canonical_dir())
    }

    fn download_file(&self, url: &Url, dest_file: &PathBuf) -> Result<(), Error> {
        let mut resp = reqwest::get(url.as_str()).expect("Unable to retrieve image from url");
        assert!(resp.status().is_success());
        let mut buffer = Vec::new();
        resp.read_to_end(&mut buffer)?;
        write(&dest_file, buffer)?;
        Ok(())
    }

    fn download_node(&mut self) -> Result<(), Error> {
        #[cfg(target_os = "windows")]
        let url = Url::parse("https://nodejs.org/dist/v12.13.1/node-v12.13.1-win-x64.zip")?;
        #[cfg(target_os = "linux")]
        let url = Url::parse("https://nodejs.org/dist/v12.13.0/node-v12.13.0-linux-x64.tar.xz")?;
        println!("Downloading {}...", &url);
        let node: PathBuf = self
            .canonical_dir()
            .join(url.path_segments().unwrap().last().unwrap());
        if node.exists() {
            println!("Skipping node download.");
            return Ok(());
        }
        self.download_file(&url, &node)?;
        self.node.url = Some(url.into_string());
        Ok(())
    }

    fn unzip(&self, zip_file: &PathBuf, dest_dir: &PathBuf) -> Result<PathBuf, Error> {
        // Zip extration taken from example in zip crate.
        let file = File::open(&zip_file).unwrap();

        let mut archive = zip::ZipArchive::new(file).unwrap();
        let root = archive.by_index(0)?.sanitized_name();

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).unwrap();
            let outpath = dest_dir.join(file.sanitized_name());

            if (&*file.name()).ends_with('/') {
                if outpath.exists() {
                    println!("Skipping extraction.");
                    break;
                }
                debug!(
                    "File {} extracted to \"{}\"",
                    i,
                    outpath.as_path().display()
                );
                create_dir_all(&outpath).unwrap();
            } else {
                debug!(
                    "File {} extracted to \"{}\" ({} bytes)",
                    i,
                    outpath.as_path().display(),
                    file.size()
                );
                if let Some(p) = outpath.parent() {
                    if !p.exists() {
                        create_dir_all(&p).unwrap();
                    }
                }
                let mut outfile = File::create(&outpath).unwrap();
                io::copy(&mut file, &mut outfile).unwrap();
            }
        }
        Ok(root)
    }

    fn extract_node(&mut self) -> Result<(), Error> {
        println!("Extracting nodejs...");
        let mut path = None;
        for entry in glob(self.canonical_dir().join("node*.*").to_str().unwrap())
            .expect("Failed to read glob pattern")
        {
            match entry {
                Ok(matching_file) => {
                    if matching_file.is_dir() {
                        continue;
                    }
                    path = Some(matching_file);
                    break;
                }
                Err(e) => panic!("Error while searching for node install: {:?}", e),
            };
        }
        if path.is_none() {
            panic!("Unable to find node install.");
        }
        let path = path.unwrap();
        if path.to_str().unwrap().contains(&".tar.gz".to_string()) {}
        if path.to_str().unwrap().contains(&".tar.xz".to_string()) {
            let tar_gz = File::open(path)?;
            let tar = XzDecoder::new(tar_gz);
            let mut archive = Archive::new(tar);
            archive.unpack(self.canonical_dir())?;
            return Ok(());
        }
        if path.to_str().unwrap().contains(&".zip".to_string()) {
            self.node.path = Some(self.unzip(&path, &self.canonical_dir())?);
            return Ok(());
        }
        Err(err_msg(format!(
            "Unrecognized archive file type: {}",
            path.display()
        )))
    }

    fn wiki_dir(&self) -> PathBuf {
        self.canonical_dir().join(self.wiki.dir())
    }

    fn client_dir(&self) -> PathBuf {
        self.canonical_dir().join(self.client.dir())
    }

    fn server_dir(&self) -> PathBuf {
        self.canonical_dir().join(self.server.dir())
    }

    fn wiki_zip(&self) -> PathBuf {
        self.canonical_dir().join(self.wiki.zip())
    }

    fn client_zip(&self) -> PathBuf {
        self.canonical_dir().join(self.client.zip())
    }

    fn server_zip(&self) -> PathBuf {
        self.canonical_dir().join(self.server.zip())
    }

    fn download_if_needed(&self, url: &Url, zip_file: &PathBuf) -> Result<(), Error> {
        println!("Downloading {}...", &url);
        if zip_file.exists() {
            println!("Skipping download.");
            return Ok(());
        }
        self.download_file(&url, &zip_file)?;
        Ok(())
    }

    fn download_wiki(&self) -> Result<(), Error> {
        self.download_if_needed(&self.wiki.url(), &self.wiki_zip())?;
        self.download_if_needed(&self.client.url(), &self.client_zip())?;
        self.download_if_needed(&self.server.url(), &self.server_zip())?;
        Ok(())
    }

    fn extract_wiki(&self) -> Result<(), Error> {
        println!("Extracting wiki...");
        self.unzip(&self.wiki_zip(), &self.canonical_dir())?;
        self.unzip(&self.client_zip(), &self.canonical_dir())?;
        self.unzip(&self.server_zip(), &self.canonical_dir())?;
        Ok(())
    }

    fn run_npm(&self, dir: &PathBuf, args: &[&str]) -> Result<(), Error> {
        #[cfg(target_os = "windows")]
        let npm = "npm.cmd".to_owned();
        #[cfg(target_os = "linux")]
        let npm = "npm".to_owned();
        let command_path = self
            .canonical_dir()
            .join(
                self.node
                    .path
                    .as_ref()
                    .expect("Unable to run NPM; No path to node!"),
            )
            .join(npm);
        println!("NPM path: {}", command_path.display());
        let mut command = Command::new(&command_path);
        command.arg("--scripts-prepend-node-path=true");
        for arg in args {
            command.arg(arg);
        }
        println!("Running {:?}...", command);
        let mut command = command.current_dir(dir).spawn()?;
        command.wait()?;
        Ok(())
    }

    fn link_dep(&self, dir: &PathBuf) -> Result<(), Error> {
        self.run_npm(dir, &["install"])?;
        self.run_npm(dir, &["link", self.client_dir().to_str().unwrap()])?;
        Ok(())
    }

    fn install_wiki(&self) -> Result<(), Error> {
        println!("Installing wiki...");
        if self.node.path.is_none() {
            eprintln!("Node installation not found, aborting.");
            exit(1);
        }
        self.link_dep(&self.client_dir())?;
        self.link_dep(&self.server_dir())?;
        self.run_npm(&self.wiki_dir(), &["install"])?;
        Ok(())
    }

    fn create_wiki(&mut self) -> Result<(), Error> {
        self.create_folder()?;
        self.download_node()?;
        self.extract_node()?;
        self.download_wiki()?;
        self.extract_wiki()?;
        self.install_wiki()?;
        self.save()?;
        Ok(())
    }

    fn save(&self) -> Result<(), Error> {
        let file = File::create(self.canonical_dir().join("config.yaml"))?;
        serde_yaml::to_writer(file, self)?;
        Ok(())
    }

    fn run_wiki(&self) -> Result<(), Error> {
        self.run_npm(
            &self.wiki_dir(),
            &[
                "start",
                "--",
                "--security_type",
                "friends",
                "--cookie_secret",
                "a secret",
                "--farm",
            ],
        )?;
        Ok(())
    }

    fn delete(&self) -> Result<(), Error> {
        println!("Deleting wiki install...");
        if self.exists() {
            remove_dir_all(self.canonical_dir())?;
        }
        Ok(())
    }

    fn delete_node(&self) -> Result<(), Error> {
        println!("Deleting node...");
        if self.node.path.is_some() {
            let node_path = self.node.path.as_ref().unwrap();
            if node_path.exists() {
                remove_dir_all(node_path)?;
            }
        }
        Ok(())
    }

    fn delete_wiki(&self) -> Result<(), Error> {
        self.delete_wiki_repo()?;
        self.delete_server_repo()?;
        self.delete_client_repo()?;
        Ok(())
    }

    fn delete_wiki_repo(&self) -> Result<(), Error> {
        println!("Deleting wiki repo...");
        if self.wiki_dir().exists() {
            remove_dir_all(self.wiki_dir())?;
        }
        Ok(())
    }

    fn delete_server_repo(&self) -> Result<(), Error> {
        println!("Deleting wiki-server repo...");
        if self.server_dir().exists() {
            remove_dir_all(self.server_dir())?;
        }
        Ok(())
    }

    fn delete_client_repo(&self) -> Result<(), Error> {
        println!("Deleting wiki-client repo...");
        if self.client_dir().exists() {
            remove_dir_all(self.client_dir())?;
        }
        Ok(())
    }
}

fn run() -> Result<(), Error> {
    let matches = App::new("wiki-create")
        .about("Utility to create a mostly self-contained wiki install.")
        .setting(AppSettings::ArgRequiredElseHelp)
        .arg(
            Arg::with_name("config")
                .long("config")
                .help("Use the config")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("dir")
                .long("dir")
                .help("Directory in which to create the wiki")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("update")
                .long("update")
                .help("Update existing wiki"),
        )
        .arg(
            Arg::with_name("clean-all")
                .long("clean-all")
                .help("Delete the entire wiki site"),
        )
        .arg(
            Arg::with_name("clean")
                .long("clean")
                .help("Delete the wiki install"),
        )
        .arg(
            Arg::with_name("clean-wiki")
                .long("clean-wiki")
                .help("Delete the wiki repository"),
        )
        .arg(
            Arg::with_name("clean-server")
                .long("clean-server")
                .help("Delete the wiki-server repository"),
        )
        .arg(
            Arg::with_name("clean-client")
                .long("clean-client")
                .help("Delete the wiki-client repository"),
        )
        .arg(
            Arg::with_name("clean-plugin")
                .long("clean-plugin")
                .takes_value(true)
                .help("Delete the repository for the named plugin"),
        )
        .arg(
            Arg::with_name("clean-node")
                .long("clean-node")
                .help("Delete only the node install"),
        )
        .arg(Arg::with_name("run").long("run").help("Run the wiki"))
        .get_matches();
    let mut config: WikiConfig = if matches.is_present("config") {
        let config_path = matches.value_of("config").unwrap();
        WikiConfig::from_config(config_path).expect("Unable to load specified config")
    } else if matches.is_present("dir") {
        let dir: PathBuf = matches.value_of("dir").unwrap().into();
        let config_path = dir.join("config.yaml");
        let maybe_config = WikiConfig::from_config(&config_path.to_str().unwrap());
        match maybe_config {
            Err(err) => {
                if config_path.exists() {
                    return Err(err_msg(format!(
                        "Unable to parse config file: {}\n{}",
                        config_path.display(),
                        err
                    )));
                }
                eprintln!("No wiki config file found: {}.", config_path.display());
                WikiConfig::new(dir.to_str().unwrap())
            }
            Ok(config) => config,
        }
    } else {
        exit(1);
    };
    if matches.is_present("clean-all") {
        config.delete()?;
    }
    if matches.is_present("clean") {
        config.delete_wiki()?;
    }
    if matches.is_present("clean-wiki") {
        config.delete_wiki_repo()?;
    }
    if matches.is_present("clean-server") {
        config.delete_server_repo()?;
    }
    if matches.is_present("clean-client") {
        config.delete_client_repo()?;
    }
    if matches.is_present("clean-node") {
        config.delete_node()?;
    }
    if !config.exists() || matches.is_present("update") {
        config.create_wiki()?;
    }
    if matches.is_present("run") {
        config.run_wiki()?;
    }
    Ok(())
}

fn main() {
    match run() {
        Ok(_) => (),
        Err(err) => {
            eprintln!("{}", err);
            exit(1);
        }
    }
}
