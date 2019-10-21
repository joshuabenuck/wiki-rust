use clap::{App, Arg};
use failure::Error;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use wiki_rust::{Page, Sitemap};

// Consider submitting a PR against the webbrowser crate
// https://github.com/amodm/webbrowser-rs

// The approached used here differs from that used by the crate on Windows.
// It originated from a web search: windows open web browser command line
// Type "start iexplore" and press "Enter" to open Internet Explorer
// and view its default home screen. Alternatively, type "start firefox,"
// "start opera" or "start chrome" and press "Enter" to open one of those browsers.
fn open_browser(url: &str) {
    let _output = if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(&["/C", format!("start chrome {}", url).as_str()])
            .output()
            .expect("failed to execute process")
    } else {
        Command::new("sh")
            .arg("-c")
            .arg("echo hello")
            .output()
            .expect("failed to execute process")
    };
}

fn main() -> Result<(), Error> {
    let matches = App::new("wiki-print")
        .about("Formats a federated wiki site for printing.")
        .arg(
            Arg::with_name("site")
                .long("site")
                .short("s")
                .required(true)
                .takes_value(true)
                .help("The site to format."),
        )
        .get_matches();
    let site = matches
        .value_of("site")
        .expect("Unable to get value for site");
    let mut file = fs::File::create("site.html")?;
    writeln!(
        file,
        "<html>
            <head></head>
            <body>
    "
    )?;
    let sitemap = Sitemap::from_url(site).expect("Unable to retrieve or parse sitemap!");
    for entry in sitemap.entries {
        let page = Page::from_site_slug(site, &entry.slug)?;
        writeln!(file, "<div class=\"page\"><div>{}<div>", page.title)?;
        writeln!(file, "<div class=\"story\">")?;
        for item in page.story {
            writeln!(file, "<div class=\"item\">{}</div>", item.text.unwrap())?;
        }
        writeln!(file, "</div></div>")?;
        break;
    }
    writeln!(
        file,
        "   </body>
        </html>
    "
    )?;
    drop(file);
    open_browser(
        format!(
            "file://{}/site.html",
            PathBuf::from(".").canonicalize().unwrap().display()
        )
        .as_str(),
    );
    Ok(())
}
