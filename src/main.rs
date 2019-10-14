use chrono::{Duration, NaiveDateTime, Utc};
use clap::{App, Arg, ArgMatches};
use failure::{err_msg, Error};
use reqwest;
use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json;
use std::convert::TryInto;
use std::time;
use url::Url;

fn de_from_u64<'de, D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    let d = u64::deserialize(deserializer)?;
    Ok(NaiveDateTime::from_timestamp(
        (d / 1000) as i64,
        time::Duration::from_millis(d).subsec_nanos(),
    ))
}

#[derive(Deserialize)]
struct Entry {
    slug: String,
    title: String,
    #[serde(deserialize_with = "de_from_u64")]
    date: NaiveDateTime,
    synopsis: String,
}

struct Sitemap {
    name: String,
    entries: Vec<Entry>,
}

impl Sitemap {
    fn from_url(url: &str) -> Result<Sitemap, Error> {
        let parsed_url = Url::parse(&url).unwrap().join("/system/sitemap.json")?;
        println!("Parsing sitemap: {}", &parsed_url);
        let mut response = reqwest::get(parsed_url.as_str())?;
        let mut entries: Vec<Entry> = response.json()?;
        entries.sort_unstable_by_key(|e| e.date);
        entries.reverse();
        Ok(Sitemap {
            name: parsed_url.host_str().unwrap().to_owned(),
            entries,
        })
    }
}

struct Neighborhood {
    sites: Vec<Sitemap>,
}

impl Neighborhood {
    fn new() -> Neighborhood {
        Neighborhood { sites: Vec::new() }
    }

    fn add(&mut self, url: &str) -> Result<&mut Self, Error> {
        self.sites.push(Sitemap::from_url(&url)?);
        Ok(self)
    }
}

#[derive(Deserialize)]
struct Item {
    r#type: String,
    id: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct Change {
    r#type: String,
    #[serde(deserialize_with = "de_from_u64")]
    date: NaiveDateTime,
}

#[derive(Deserialize)]
struct Page {
    title: String,
    story: Vec<Item>,
    journal: Vec<Change>,
}

impl Page {
    fn from_site_slug(site_name: &str, slug: &str) -> Result<Page, Error> {
        let parsed_url = Url::parse(format!("http://{}/{}.json", &site_name, slug).as_str())?;
        println!("Loading: {}", parsed_url);
        let mut response = reqwest::get(parsed_url.as_str())?;
        Ok(response.json()?)
    }
}

fn run(matches: &ArgMatches) -> Result<(), Error> {
    let mut sites = Vec::<Sitemap>::new();
    if matches.is_present("pod") {
        let site_filter = matches.value_of("site");
        let page = Page::from_site_slug("code.fed.wiki", "our-learning-pod")?;
        let mut neighborhood = Neighborhood::new();
        for item in page.story {
            if item.r#type == "roster" {
                for line in item.text.unwrap().split("\n") {
                    let line = line.trim();
                    if line.len() == 0 || line.contains("Our Learning Pod") {
                        continue;
                    }
                    if let Some(site) = site_filter {
                        if !line.contains(site) {
                            continue;
                        }
                    }
                    neighborhood.add(format!("http://{}", line).as_str())?;
                }
            }
        }
        for site in neighborhood.sites {
            sites.push(site);
        }
    } else if let Some(site) = matches.value_of("site") {
        sites.push(Sitemap::from_url(
            Url::parse(format!("http://{}", site).as_str())?.as_str(),
        )?);
    }
    let days_filter = matches.value_of("days");
    for site in sites {
        println!("{}", site.name);
        for entry in site.entries {
            if let Some(days) = days_filter {
                if Utc::now().naive_utc() - Duration::days(days.parse::<i64>().unwrap())
                    > entry.date
                {
                    continue;
                }
            }
            println!("\t{}", entry.title);
        }
    }
    Ok(())
}

fn main() {
    let matches = App::new("wiki-changes")
        .about("Get recent changes for fed wiki sites.")
        .arg(
            Arg::with_name("pod")
                .long("pod")
                .short("p")
                .takes_value(true)
                .help("Look for changes in the learning pod."),
        )
        .arg(
            Arg::with_name("site")
                .long("site")
                .short("s")
                .takes_value(true)
                .help("Look for changes in the specified site."),
        )
        .arg(
            Arg::with_name("days")
                .long("days")
                .short("d")
                .takes_value(true)
                .help("Only retrieve changes within the number of days specified."),
        )
        .get_matches();
    if let Err(err) = run(&matches) {
        eprintln!("{}", err);
    }
}
