use clap::{App, AppSettings, Arg};
use failure::{err_msg, Error};
use glob::glob;
use log::debug;
use serde::{Deserialize, Serialize};
use serde_yaml;
use std::fs::{create_dir_all, remove_dir_all, write, File};
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{exit, Command};
use tar::Archive;
use url::Url;
use xz2::read::XzDecoder;

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
    wiki: PathBuf,
    #[serde(default, skip_serializing_if = "is_default")]
    server: PathBuf,
    #[serde(default, skip_serializing_if = "is_default")]
    client: PathBuf,
    #[serde(default, skip_serializing_if = "is_default")]
    plugins: Vec<PathBuf>,
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

    fn wiki_url(&self) -> Url {
        Url::parse("https://github.com/joshuabenuck/wiki/archive/master.zip")
            .expect("Unable to parse wiki url")
    }

    fn wiki_client_url(&self) -> Url {
        Url::parse("https://github.com/joshuabenuck/wiki-client/archive/master.zip")
            .expect("Unable to parse wiki client url")
    }

    fn wiki_server_url(&self) -> Url {
        Url::parse("https://github.com/joshuabenuck/wiki-server/archive/master.zip")
            .expect("Unable to parse wiki server url")
    }

    fn wiki_zip(&self) -> PathBuf {
        self.canonical_dir().join("wiki.zip")
    }

    fn wiki_client_zip(&self) -> PathBuf {
        self.canonical_dir().join("wiki-client.zip")
    }

    fn wiki_server_zip(&self) -> PathBuf {
        self.canonical_dir().join("wiki-server.zip")
    }

    fn wiki_path(&self) -> PathBuf {
        // TODO: Compute or get path name from a cache...
        self.canonical_dir().join("wiki-master")
    }

    fn wiki_client_path(&self) -> PathBuf {
        // TODO: Compute or get path name from a cache...
        self.canonical_dir().join("wiki-client-master")
    }

    fn wiki_server_path(&self) -> PathBuf {
        // TODO: Compute or get path name from a cache...
        self.canonical_dir().join("wiki-server-master")
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
        self.download_if_needed(&self.wiki_url(), &self.wiki_zip())?;
        self.download_if_needed(&self.wiki_client_url(), &self.wiki_client_zip())?;
        self.download_if_needed(&self.wiki_server_url(), &self.wiki_server_zip())?;
        Ok(())
    }

    fn extract_wiki(&self) -> Result<(), Error> {
        println!("Extracting wiki...");
        let wiki_zip = self.wiki_zip();
        self.unzip(&wiki_zip, &self.canonical_dir())?;
        let wiki_client_zip = self.canonical_dir().join("wiki-client.zip");
        self.unzip(&wiki_client_zip, &self.canonical_dir())?;
        let wiki_server_zip = self.canonical_dir().join("wiki-server.zip");
        self.unzip(&wiki_server_zip, &self.canonical_dir())?;
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

    fn install_wiki(&self) -> Result<(), Error> {
        println!("Installing wiki...");
        if self.node.path.is_none() {
            eprintln!("Node installation not found, aborting.");
            exit(1);
        }
        self.run_npm(&self.wiki_client_path(), &["install"])?;
        self.run_npm(&self.wiki_server_path(), &["install"])?;
        self.run_npm(
            &self.wiki_path(),
            &["link", self.wiki_client_path().to_str().unwrap()],
        )?;
        self.run_npm(
            &self.wiki_path(),
            &["link", self.wiki_server_path().to_str().unwrap()],
        )?;
        self.run_npm(&self.wiki_path(), &["install"])?;
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
            &self.wiki_path(),
            &[
                "start",
                "--",
                "--security_type",
                "friends",
                "--cookie_secret",
                "a secret",
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
        println!("Deleting wiki...");
        let wiki_path = self.canonical_dir().join("wiki-master");
        if wiki_path.exists() {
            remove_dir_all(wiki_path)?;
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
            Arg::with_name("clean")
                .long("clean")
                .help("Delete the wiki config and start anew"),
        )
        .arg(
            Arg::with_name("clean-wiki")
                .long("clean-wiki")
                .help("Delete only the wiki install"),
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
    if matches.is_present("clean") {
        config.delete()?;
    }
    if matches.is_present("clean-wiki") {
        config.delete_wiki()?;
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
