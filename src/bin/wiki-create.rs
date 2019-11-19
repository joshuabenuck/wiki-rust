use clap::{App, AppSettings, Arg};
use failure::Error;
use serde::{Deserialize, Serialize};
use serde_yaml;
use std::fs::{create_dir_all, write, File};
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{exit, Command};
use tar::Archive;
use xz2::read::XzDecoder;

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
        }
    }
}

impl WikiConfig {
    fn from_config(config: &str) -> WikiConfig {
        WikiConfig {
            ..WikiConfig::default()
        }
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
        self.canonical_dir().exists()
    }

    fn create_folder(&self) -> Result<(), io::Error> {
        create_dir_all(self.canonical_dir())
    }

    fn download_node(&self) -> Result<(), Error> {
        println!("Downloading nodejs...");
        let node: PathBuf = self.canonical_dir().join("nodejs.tar.xz");
        let url = "https://nodejs.org/dist/v12.13.0/node-v12.13.0-linux-x64.tar.xz";
        let mut resp = reqwest::get(url).expect("Unable to retrieve image from url");
        assert!(resp.status().is_success());
        let mut buffer = Vec::new();
        resp.read_to_end(&mut buffer)?;
        write(&node, buffer)?;
        Ok(())
    }

    fn extract_node(&self) -> Result<(), Error> {
        println!("Extracting nodejs...");
        let tar_gz = File::open(self.canonical_dir().join("nodejs.tar.xz"))?;
        let tar = XzDecoder::new(tar_gz);
        let mut archive = Archive::new(tar);
        archive.unpack(".")?;
        Ok(())
    }

    fn clone_wiki(&self) {}

    fn create_wiki(&self) -> Result<(), Error> {
        self.create_folder()?;
        self.download_node()?;
        self.extract_node()?;
        Ok(())
    }
}

fn main() -> Result<(), Error> {
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
        .get_matches();
    let config: WikiConfig = if matches.value_of("config").is_some() {
        let reader = File::open(matches.value_of("config").unwrap())?;
        serde_yaml::from_reader(reader)?
    } else if matches.value_of("dir").is_some() {
        WikiConfig::new(matches.value_of("dir").unwrap())
    } else {
        exit(1);
    };
    if config.exists() && !matches.is_present("update") {
        println!("Refusing to update existing wiki. Pass --update to force.");
        println!("WARNING: All content of directory will be erased!");
        exit(1);
    }
    config.create_wiki();
    Ok(())
}
