use chrono::{Duration, Utc};
use clap::{App, Arg, ArgMatches};
use failure::Error;
use url::Url;
use wiki_rust::{Neighborhood, Page, Sitemap};

fn run(matches: &ArgMatches) -> Result<(), Error> {
    let mut sites = Vec::<Sitemap>::new();
    if matches.is_present("pod") {
        let site_filter = matches.value_of("site");
        let page = Page::from_site_slug("http://code.fed.wiki", "our-learning-pod")?;
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
