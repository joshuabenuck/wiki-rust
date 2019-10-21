use chrono::NaiveDateTime;
use failure::Error;
use reqwest;
use serde::{Deserialize, Deserializer};
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
pub struct Entry {
    pub slug: String,
    pub title: String,
    #[serde(deserialize_with = "de_from_u64")]
    pub date: NaiveDateTime,
    pub synopsis: String,
}

pub struct Sitemap {
    pub name: String,
    pub entries: Vec<Entry>,
}

impl Sitemap {
    pub fn from_url(url: &str) -> Result<Sitemap, Error> {
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

pub struct Neighborhood {
    pub sites: Vec<Sitemap>,
}

impl Neighborhood {
    pub fn new() -> Neighborhood {
        Neighborhood { sites: Vec::new() }
    }

    pub fn add(&mut self, url: &str) -> Result<&mut Self, Error> {
        self.sites.push(Sitemap::from_url(&url)?);
        Ok(self)
    }
}

#[derive(Deserialize)]
pub struct Item {
    pub r#type: String,
    pub id: String,
    pub text: Option<String>,
}

#[derive(Deserialize)]
pub struct Change {
    pub r#type: String,
    #[serde(deserialize_with = "de_from_u64")]
    pub date: NaiveDateTime,
}

#[derive(Deserialize)]
pub struct Page {
    pub title: String,
    pub story: Vec<Item>,
    pub journal: Vec<Change>,
}

impl Page {
    pub fn from_site_slug(site_name: &str, slug: &str) -> Result<Page, Error> {
        let parsed_url = Url::parse(format!("{}/{}.json", &site_name, slug).as_str())?;
        println!("Loading: {}", parsed_url);
        let mut response = reqwest::get(parsed_url.as_str())?;
        Ok(response.json()?)
    }
}
